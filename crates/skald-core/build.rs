use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SKALD_BUILD_ACCELERATION");
    println!("cargo:rerun-if-env-changed=SKALD_RELEASE_TARGET");
    println!("cargo:rerun-if-env-changed=SKALD_CUDA_TARGET");
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=SKALD_BUILD_TARGET={target}");
    }

    if let Some(commit) = command_stdout("git", &["rev-parse", "--short=12", "HEAD"]) {
        println!("cargo:rustc-env=SKALD_BUILD_COMMIT={commit}");
    }

    if let Some(tag) = command_stdout("git", &["describe", "--tags", "--exact-match"]) {
        println!("cargo:rustc-env=SKALD_BUILD_TAG={tag}");
    }

    if let Some(text) = command_stdout("rustc", &["-Vv"]) {
        if let Some(host) = text
            .lines()
            .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        {
            println!("cargo:rustc-env=SKALD_BUILD_RUST_HOST={host}");
        }
        if let Some(version) = text.lines().next() {
            println!("cargo:rustc-env=SKALD_BUILD_RUSTC={version}");
        }
    }
}

fn command_stdout(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
