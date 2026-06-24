use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    pub commit: &'static str,
    pub tag: &'static str,
    pub target: &'static str,
    pub rustc: &'static str,
    pub rust_host: &'static str,
    pub acceleration: &'static str,
    pub cuda_target: &'static str,
}

#[must_use]
pub fn build_info(acceleration: &'static str) -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("SKALD_BUILD_COMMIT").unwrap_or("unknown"),
        tag: option_env!("SKALD_BUILD_TAG").unwrap_or("unreleased"),
        target: option_env!("SKALD_RELEASE_TARGET")
            .or(option_env!("SKALD_BUILD_TARGET"))
            .unwrap_or("unknown"),
        rustc: option_env!("SKALD_BUILD_RUSTC").unwrap_or("unknown"),
        rust_host: option_env!("SKALD_BUILD_RUST_HOST").unwrap_or("unknown"),
        acceleration,
        cuda_target: option_env!("SKALD_CUDA_TARGET").unwrap_or("none"),
    }
}
