#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::needless_pass_by_value
)]

use std::{
    cell::RefCell,
    collections::VecDeque,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use clap::Parser;
use gtk::{
    Application, ApplicationWindow, Box as GtkBox, DrawingArea, Label, Orientation, glib,
    prelude::*,
};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use skald_core::{
    client::{self, overlay_event_kinds},
    config::Config,
    preview::dedupe_preview_parts,
    protocol::{Event, JobState},
};
use skald_platform::{OverlayPlacementHint, capture_overlay_placement_hint, overlay_session_hint};
use tracing::{info, warn};

#[derive(Debug, Parser)]
#[command(name = "skald-overlay", about = "Skald dictation preview overlay")]
struct Args {
    #[arg(long)]
    socket: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum UiMessage {
    Connected,
    Disconnected,
    Event(Box<Event>),
}

#[derive(Debug, Default)]
struct OverlayState {
    status: String,
    stable: String,
    provisional: String,
    visible: bool,
    recording: bool,
    last_markup: String,
    waveform: VecDeque<f64>,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = Config::load_or_default()?;
    config.validate()?;
    let socket = args
        .socket
        .unwrap_or_else(|| client::socket_path_from_config().expect("socket path"));
    let overlay_config = config.overlay.clone();
    let hide_when_idle = overlay_config.hide_when_idle;
    let hint = overlay_session_hint();
    info!(
        session = hint.id,
        detail = hint.detail,
        "overlay session hint"
    );

    let (ui_tx, ui_rx) = mpsc::channel::<UiMessage>();
    spawn_event_worker(socket, ui_tx);

    let app = Application::builder()
        .application_id("dev.skald.Overlay")
        .build();
    let ui_rx = Rc::new(RefCell::new(Some(ui_rx)));
    app.connect_activate(move |app| {
        let Some(rx) = ui_rx.borrow_mut().take() else {
            return;
        };
        build_ui(app, rx, overlay_config.clone(), hide_when_idle, hint);
    });
    app.run();
    Ok(())
}

fn spawn_event_worker(socket: PathBuf, ui_tx: mpsc::Sender<UiMessage>) {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("overlay tokio runtime should start");
        runtime.block_on(async move {
            let kinds = overlay_event_kinds();
            let mut backoff = Duration::from_secs(1);
            loop {
                match client::subscribe(&socket, kinds.clone()).await {
                    Ok((response, reader)) => {
                        if response.ok {
                            let _ = ui_tx.send(UiMessage::Connected);
                            backoff = Duration::from_secs(1);
                            let mut reader = tokio::io::BufReader::new(reader);
                            loop {
                                match client::read_event(&mut reader).await {
                                    Ok(event) => {
                                        if ui_tx.send(UiMessage::Event(Box::new(event))).is_err() {
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        warn!(%error, "event stream ended");
                                        break;
                                    }
                                }
                            }
                        } else {
                            warn!("subscribe rejected by daemon");
                        }
                    }
                    Err(error) => {
                        warn!(%error, "failed to connect to daemon");
                    }
                }
                let _ = ui_tx.send(UiMessage::Disconnected);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(15));
            }
        });
    });
}

#[allow(clippy::too_many_lines)]
fn build_ui(
    app: &Application,
    ui_rx: mpsc::Receiver<UiMessage>,
    overlay_config: skald_core::config::OverlayConfig,
    hide_when_idle: bool,
    hint: skald_platform::OverlaySessionHint,
) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Skald")
        .default_width(overlay_config.max_width_px as i32)
        .decorated(false)
        .resizable(false)
        .build();

    let root = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(6)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(16)
        .margin_end(16)
        .build();
    root.add_css_class("skald-overlay");

    let status = Label::builder().xalign(0.0).build();
    status.add_css_class("skald-overlay-status");
    let preview = Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Word)
        .build();
    let visualizer = DrawingArea::builder()
        .height_request(72)
        .hexpand(true)
        .build();
    let drawn_waveform = Rc::new(RefCell::new(VecDeque::new()));
    let drawn_waveform_for_draw = Rc::clone(&drawn_waveform);
    let visualizer_style = overlay_config.visualizer_style.clone();
    visualizer.set_draw_func(move |_, context, width, height| {
        draw_visualizer(
            context,
            width,
            height,
            &drawn_waveform_for_draw.borrow(),
            &visualizer_style,
        );
    });
    preview.add_css_class("skald-overlay-preview");
    root.append(&status);
    root.append(&preview);
    root.append(&visualizer);
    window.set_child(Some(&root));

    apply_overlay_css(&window);

    let layer_shell = overlay_config.use_layer_shell && hint.layer_shell_recommended;
    let use_cursor_placement = overlay_config.anchor == "auto";
    if layer_shell {
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("skald-overlay"));
        window.set_keyboard_mode(KeyboardMode::None);
        window.set_exclusive_zone(-1);
        if use_cursor_placement {
            apply_cursor_layer_placement(&window, &overlay_config, None);
        } else {
            apply_screen_edge_layer_placement(&window, &overlay_config);
        }
    } else if hint.id == "gnome_wayland" {
        warn!(
            "GNOME Wayland does not support layer-shell overlays; using a floating window. \
             Positioning may be limited. Use `skald watch` as a fallback."
        );
    }

    let mut state = OverlayState {
        status: if hint.id == "gnome_wayland" {
            "Skald (limited on GNOME Wayland)".into()
        } else {
            "Skald".into()
        },
        ..OverlayState::default()
    };
    apply_state(
        &window,
        &status,
        &preview,
        &visualizer,
        &drawn_waveform,
        &overlay_config.mode,
        &mut state,
    );

    let placement_polling = Arc::new(AtomicBool::new(false));
    let (placement_tx, placement_rx) = mpsc::channel::<OverlayPlacementHint>();
    let placement_polling_for_worker = Arc::clone(&placement_polling);
    thread::spawn(move || {
        loop {
            if placement_polling_for_worker.load(Ordering::Relaxed) {
                if let Some(hint) = capture_overlay_placement_hint()
                    && placement_tx.send(hint).is_err()
                {
                    return;
                }
                thread::sleep(Duration::from_millis(250));
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
    });

    let window_for_tick = window.clone();
    let status_for_tick = status.clone();
    let preview_for_tick = preview.clone();
    let visualizer_for_tick = visualizer.clone();
    let drawn_waveform_for_tick = Rc::clone(&drawn_waveform);
    let overlay_config_for_tick = overlay_config.clone();
    let placement_polling_for_tick = Arc::clone(&placement_polling);
    let mut latest_placement: Option<OverlayPlacementHint> = None;
    let mut last_applied_placement: Option<OverlayPlacementHint> = None;
    glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(message) = ui_rx.try_recv() {
            match message {
                UiMessage::Connected => {
                    state.status = "Connected".into();
                }
                UiMessage::Disconnected => {
                    state.status = "Reconnecting…".into();
                    state.stable.clear();
                    state.provisional.clear();
                    state.recording = false;
                    state.last_markup.clear();
                    state.waveform.clear();
                    state.visible = !hide_when_idle;
                }
                UiMessage::Event(event) => apply_event(&mut state, *event, hide_when_idle),
            }
        }
        placement_polling_for_tick.store(
            layer_shell && use_cursor_placement && state.recording && state.visible,
            Ordering::Relaxed,
        );
        while let Ok(hint) = placement_rx.try_recv() {
            latest_placement = Some(hint);
        }
        if layer_shell
            && use_cursor_placement
            && state.recording
            && state.visible
            && let Some(hint) = latest_placement
            && last_applied_placement != Some(hint)
        {
            apply_cursor_layer_placement(&window_for_tick, &overlay_config_for_tick, Some(hint));
            last_applied_placement = Some(hint);
        }
        apply_state(
            &window_for_tick,
            &status_for_tick,
            &preview_for_tick,
            &visualizer_for_tick,
            &drawn_waveform_for_tick,
            &overlay_config_for_tick.mode,
            &mut state,
        );
        glib::ControlFlow::Continue
    });

    if !hide_when_idle {
        window.present();
    }
}

fn apply_overlay_css(window: &ApplicationWindow) {
    let display = gtk::prelude::WidgetExt::display(window);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        r"
        window.skald-overlay-window {
            background-color: alpha(#111111, 0.88);
            border-radius: 10px;
        }
        .skald-overlay {
            background-color: transparent;
        }
        .skald-overlay-status {
            color: #8be9fd;
            font-weight: 600;
        }
        .skald-overlay-preview {
            color: #f8f8f2;
            font-size: 15px;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    window.add_css_class("skald-overlay-window");
}

fn apply_event(state: &mut OverlayState, event: Event, hide_when_idle: bool) {
    match event {
        Event::State { job_state, .. } => {
            state.status = job_state_label(&job_state).into();
            state.recording = matches!(job_state, JobState::Recording);
            state.visible = !hide_when_idle
                || matches!(
                    job_state,
                    JobState::Recording
                        | JobState::Stopping
                        | JobState::Transcribing
                        | JobState::Cleaning
                        | JobState::Copying
                        | JobState::Injecting
                );
            if matches!(
                job_state,
                JobState::Idle | JobState::Done | JobState::Cancelled
            ) {
                state.stable.clear();
                state.provisional.clear();
                state.last_markup.clear();
                state.waveform.clear();
            }
        }
        Event::Preview {
            stable,
            provisional,
            ..
        } => {
            state.visible = true;
            state.recording = true;
            let deduped = dedupe_preview_parts(&stable, &provisional);
            state.stable = deduped.stable;
            state.provisional = deduped.provisional;
            if state.status == "Connected" {
                state.status = "Recording".into();
            }
        }
        Event::Result { .. } => {
            state.status = "Done".into();
            state.stable.clear();
            state.provisional.clear();
            state.last_markup.clear();
            state.waveform.clear();
            state.visible = true;
        }
        Event::Error { error, .. } => {
            state.status = format!("Error: {}", error.message);
            state.stable.clear();
            state.provisional.clear();
            state.last_markup.clear();
            state.waveform.clear();
            state.visible = true;
        }
        Event::AudioLevel { rms, peak, .. } => {
            state.visible = true;
            state.recording = true;
            state.waveform.push_back(normalized_audio_level(rms, peak));
            while state.waveform.len() > 96 {
                state.waveform.pop_front();
            }
            if state.status == "Connected" {
                state.status = "Recording".into();
            }
        }
    }
}

fn job_state_label(job_state: &JobState) -> &'static str {
    match job_state {
        JobState::Idle => "Idle",
        JobState::Recording => "Recording",
        JobState::Stopping => "Stopping",
        JobState::Transcribing => "Transcribing",
        JobState::Cleaning => "Cleaning",
        JobState::Copying => "Copying",
        JobState::Injecting => "Injecting",
        JobState::Done => "Done",
        JobState::Cancelled => "Cancelled",
        JobState::Failed { .. } => "Failed",
    }
}

fn apply_state(
    window: &ApplicationWindow,
    status: &Label,
    preview: &Label,
    visualizer: &DrawingArea,
    drawn_waveform: &RefCell<VecDeque<f64>>,
    mode: &str,
    state: &mut OverlayState,
) {
    status.set_text(&state.status);
    let markup = format_preview_markup(&state.stable, &state.provisional);
    if markup != state.last_markup {
        preview.set_markup(&markup);
        state.last_markup = markup;
        window.queue_resize();
    }
    let visualizer_mode = mode == "visualizer";
    preview.set_visible(!visualizer_mode);
    visualizer.set_visible(visualizer_mode);
    drawn_waveform.borrow_mut().clone_from(&state.waveform);
    visualizer.queue_draw();
    if state.visible {
        window.present();
    } else {
        window.set_visible(false);
    }
}

fn normalized_audio_level(rms: f32, peak: f32) -> f64 {
    let rms = f64::from(rms.clamp(0.0, 1.0));
    let peak = f64::from(peak.clamp(0.0, 1.0));
    let rms_db = 20.0 * rms.max(0.000_1).log10();
    let normalized_rms = ((rms_db + 52.0) / 44.0).clamp(0.0, 1.0);
    let normalized_peak = (peak / 0.5).clamp(0.0, 1.0);
    (normalized_rms * 0.8 + normalized_peak * 0.2).clamp(0.0, 1.0)
}

fn draw_visualizer(
    context: &gtk::cairo::Context,
    width: i32,
    height: i32,
    waveform: &VecDeque<f64>,
    style: &str,
) {
    match style {
        "bars" => draw_bars(context, width, height, waveform),
        "pulse" => draw_pulse(context, width, height, waveform),
        "dots" => draw_dots(context, width, height, waveform),
        _ => draw_waveform(context, width, height, waveform),
    }
}

fn draw_waveform(context: &gtk::cairo::Context, width: i32, height: i32, waveform: &VecDeque<f64>) {
    let width = f64::from(width);
    let height = f64::from(height);
    let center = height / 2.0;
    let spacing = 6.0;
    let visible_bars = ((width / spacing).floor() as usize).max(1);
    let skip = waveform.len().saturating_sub(visible_bars);
    let left_padding = (width - spacing * visible_bars as f64).max(0.0) / 2.0;

    context.set_source_rgb(0.96, 0.96, 0.96);
    context.set_line_width(2.5);
    context.set_line_cap(gtk::cairo::LineCap::Round);
    for (index, level) in waveform.iter().skip(skip).enumerate() {
        let x = left_padding + spacing * (index as f64 + 0.5);
        let half_height = (2.0 + level * (center - 5.0)).clamp(2.0, center - 2.0);
        context.move_to(x, center - half_height);
        context.line_to(x, center + half_height);
    }
    let _ = context.stroke();
}

fn draw_bars(context: &gtk::cairo::Context, width: i32, height: i32, waveform: &VecDeque<f64>) {
    const BARS: usize = 7;
    let width = f64::from(width);
    let height = f64::from(height);
    let gap = 6.0;
    let bar_width = ((width - gap * (BARS - 1) as f64) / BARS as f64).max(3.0);
    let recent: Vec<f64> = waveform.iter().rev().take(BARS).copied().collect();
    context.set_source_rgb(0.96, 0.96, 0.96);
    for index in 0..BARS {
        let level = recent.get(BARS - 1 - index).copied().unwrap_or(0.0);
        let bar_height = (4.0 + level * (height - 8.0)).clamp(4.0, height - 4.0);
        let x = index as f64 * (bar_width + gap);
        context.rectangle(x, height - bar_height, bar_width, bar_height);
        let _ = context.fill();
    }
}

fn draw_pulse(context: &gtk::cairo::Context, width: i32, height: i32, waveform: &VecDeque<f64>) {
    let width = f64::from(width);
    let height = f64::from(height);
    let level = waveform.back().copied().unwrap_or(0.0);
    let radius = (6.0 + level * (height * 0.42)).min(height * 0.46);
    context.set_source_rgba(0.96, 0.96, 0.96, 0.18 + level * 0.28);
    context.arc(
        width / 2.0,
        height / 2.0,
        radius * 1.35,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = context.fill();
    context.set_source_rgb(0.96, 0.96, 0.96);
    context.arc(
        width / 2.0,
        height / 2.0,
        radius,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = context.fill();
}

fn draw_dots(context: &gtk::cairo::Context, width: i32, height: i32, waveform: &VecDeque<f64>) {
    let width = f64::from(width);
    let height = f64::from(height);
    let center = height / 2.0;
    let spacing = 8.0;
    let visible_dots = ((width / spacing).floor() as usize).max(1);
    let skip = waveform.len().saturating_sub(visible_dots);
    let left_padding = (width - spacing * visible_dots as f64).max(0.0) / 2.0;
    context.set_source_rgb(0.96, 0.96, 0.96);
    for (index, level) in waveform.iter().skip(skip).enumerate() {
        let x = left_padding + spacing * (index as f64 + 0.5);
        let offset = level * (center - 5.0);
        let radius = 1.5 + level * 1.5;
        context.arc(x, center - offset, radius, 0.0, std::f64::consts::TAU);
        context.arc(x, center + offset, radius, 0.0, std::f64::consts::TAU);
        let _ = context.fill();
    }
}

fn apply_screen_edge_layer_placement(
    window: &ApplicationWindow,
    overlay_config: &skald_core::config::OverlayConfig,
) {
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Bottom, false);
    if overlay_config.anchor == "bottom" {
        window.set_anchor(Edge::Bottom, true);
        window.set_margin(Edge::Bottom, overlay_config.margin_px as i32);
    } else {
        window.set_anchor(Edge::Top, true);
        window.set_margin(Edge::Top, overlay_config.margin_px as i32);
    }
}

fn apply_cursor_layer_placement(
    window: &ApplicationWindow,
    overlay_config: &skald_core::config::OverlayConfig,
    placement: Option<OverlayPlacementHint>,
) {
    let Some(placement) = placement else {
        apply_screen_edge_layer_placement(window, overlay_config);
        return;
    };
    if let Some(monitor) = gtk_monitor_for_placement(window, &placement) {
        window.set_monitor(Some(&monitor));
    }
    let margin = overlay_config.margin_px as i32;
    let width = overlay_config.max_width_px as i32;
    let local_x = placement.monitor_local_x();
    let local_y = placement.monitor_local_y();
    let left = local_x.saturating_sub(width / 2).clamp(
        margin,
        placement.monitor_width.saturating_sub(width + margin),
    );
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, false);
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Bottom, false);
    window.set_margin(Edge::Left, left);
    if placement.prefer_below_cursor() {
        window.set_anchor(Edge::Top, true);
        window.set_margin(
            Edge::Top,
            (local_y + margin).clamp(margin, placement.monitor_height.saturating_sub(margin)),
        );
    } else {
        window.set_anchor(Edge::Bottom, true);
        window.set_margin(
            Edge::Bottom,
            (placement.monitor_height - local_y + margin)
                .clamp(margin, placement.monitor_height.saturating_sub(margin)),
        );
    }
}

fn gtk_monitor_for_placement(
    window: &ApplicationWindow,
    placement: &OverlayPlacementHint,
) -> Option<gtk::gdk::Monitor> {
    let display = gtk::prelude::WidgetExt::display(window);
    let monitors = display.monitors();
    for index in 0..monitors.n_items() {
        let monitor = monitors.item(index)?.downcast::<gtk::gdk::Monitor>().ok()?;
        let geometry = monitor.geometry();
        if placement.cursor_x >= geometry.x()
            && placement.cursor_x < geometry.x() + geometry.width()
            && placement.cursor_y >= geometry.y()
            && placement.cursor_y < geometry.y() + geometry.height()
        {
            return Some(monitor);
        }
    }
    None
}

fn format_preview_markup(stable: &str, provisional: &str) -> String {
    if stable.is_empty() && provisional.is_empty() {
        return String::new();
    }
    if provisional.is_empty() {
        return glib::markup_escape_text(stable).to_string();
    }
    if stable.is_empty() {
        return format!(
            "<span alpha='75%'><i>{}</i></span>",
            glib::markup_escape_text(provisional)
        );
    }
    format!(
        "{} <span alpha='70%'><i>{}</i></span>",
        glib::markup_escape_text(stable),
        glib::markup_escape_text(provisional)
    )
}

#[cfg(test)]
mod tests {
    use super::normalized_audio_level;

    #[test]
    fn audio_level_mapping_is_bounded_and_monotonic() {
        let silence = normalized_audio_level(0.0, 0.0);
        let quiet = normalized_audio_level(0.01, 0.03);
        let speech = normalized_audio_level(0.08, 0.2);
        let loud = normalized_audio_level(1.0, 1.0);

        assert!(silence.abs() < f64::EPSILON);
        assert!(quiet > silence);
        assert!(speech > quiet);
        assert!((loud - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn audio_level_mapping_clamps_invalid_ranges() {
        assert!(normalized_audio_level(-1.0, -1.0).abs() < f64::EPSILON);
        assert!((normalized_audio_level(2.0, 2.0) - 1.0).abs() < f64::EPSILON);
    }
}
