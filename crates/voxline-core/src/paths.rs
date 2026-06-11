use std::path::PathBuf;

use crate::config::PathsConfig;

pub const STYLES_DIR: &str = "styles";
pub const APPS_DIR: &str = "apps";
pub const SNIPPETS_DIR: &str = "snippets";

#[must_use]
pub fn expand_home(path: &str) -> PathBuf {
    if let Some(relative) = path.strip_prefix("~/") {
        dirs::home_dir().map_or_else(|| PathBuf::from(path), |home| home.join(relative))
    } else if path == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    }
}

#[must_use]
pub fn resolve_config_dir(paths: &PathsConfig) -> PathBuf {
    expand_home(&paths.config_dir)
}

#[must_use]
pub fn resolve_model_dir(paths: &PathsConfig) -> PathBuf {
    expand_home(&paths.model_dir)
}

pub fn resolve_runtime_dir(paths: &PathsConfig) -> Result<PathBuf, crate::runtime::RuntimeError> {
    if paths.runtime_dir == "auto" {
        crate::runtime::xdg_runtime_dir()
    } else {
        Ok(expand_home(&paths.runtime_dir))
    }
}

#[must_use]
pub fn styles_dir(paths: &PathsConfig) -> PathBuf {
    resolve_config_dir(paths).join(STYLES_DIR)
}

#[must_use]
pub fn apps_dir(paths: &PathsConfig) -> PathBuf {
    resolve_config_dir(paths).join(APPS_DIR)
}

#[must_use]
pub fn snippets_dir(paths: &PathsConfig) -> PathBuf {
    resolve_config_dir(paths).join(SNIPPETS_DIR)
}

pub fn scaffold_config_layout(paths: &PathsConfig) -> Result<(), std::io::Error> {
    for dir in [
        resolve_config_dir(paths),
        styles_dir(paths),
        apps_dir(paths),
        snippets_dir(paths),
        resolve_model_dir(paths),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

#[must_use]
pub fn layout_is_scaffolded(paths: &PathsConfig) -> bool {
    [styles_dir(paths), apps_dir(paths), snippets_dir(paths)]
        .iter()
        .all(|dir| dir.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_creates_routing_directories() {
        let base = std::env::temp_dir().join(format!("voxline-paths-{}", ulid::Ulid::new()));
        let paths = PathsConfig {
            config_dir: base.join("config").display().to_string(),
            model_dir: base.join("models").display().to_string(),
            runtime_dir: base.join("runtime").display().to_string(),
        };
        scaffold_config_layout(&paths).unwrap();
        assert!(styles_dir(&paths).is_dir());
        assert!(apps_dir(&paths).is_dir());
        assert!(snippets_dir(&paths).is_dir());
        assert!(resolve_model_dir(&paths).is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }
}
