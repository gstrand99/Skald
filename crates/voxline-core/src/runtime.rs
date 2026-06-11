use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{config::PathsConfig, paths};

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("XDG_RUNTIME_DIR is unavailable; configure paths.runtime_dir explicitly")]
    MissingXdgRuntimeDir,
    #[error("runtime path is not owned by the current user: {0}")]
    NotOwned(PathBuf),
    #[error("runtime directory operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn xdg_runtime_dir() -> Result<PathBuf, RuntimeError> {
    let base = env::var_os("XDG_RUNTIME_DIR").ok_or(RuntimeError::MissingXdgRuntimeDir)?;
    Ok(PathBuf::from(base).join("voxline"))
}

pub fn runtime_dir() -> Result<PathBuf, RuntimeError> {
    runtime_dir_for(&PathsConfig::default())
}

pub fn runtime_dir_for(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    paths::resolve_runtime_dir(paths)
}

pub fn socket_path() -> Result<PathBuf, RuntimeError> {
    socket_path_for(&PathsConfig::default())
}

pub fn socket_path_for(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    Ok(runtime_dir_for(paths)?.join("voxlined.sock"))
}

pub fn ensure_runtime_dir() -> Result<PathBuf, RuntimeError> {
    ensure_runtime_dir_for(&PathsConfig::default())
}

pub fn ensure_runtime_dir_for(paths: &PathsConfig) -> Result<PathBuf, RuntimeError> {
    let path = runtime_dir_for(paths)?;
    fs::create_dir_all(&path).map_err(|source| RuntimeError::Io {
        path: path.clone(),
        source,
    })?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        RuntimeError::Io {
            path: path.clone(),
            source,
        }
    })?;
    verify_mode(&path)?;
    Ok(path)
}

pub fn verify_mode(path: &Path) -> Result<(), RuntimeError> {
    let metadata = fs::metadata(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(RuntimeError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "runtime directory must have mode 0700",
            ),
        });
    }
    Ok(())
}

pub fn secure_socket_permissions(path: &Path) -> Result<(), RuntimeError> {
    if !path.exists() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        RuntimeError::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    verify_socket(path)
}

#[must_use]
pub fn socket_permissions_ok(path: &Path) -> bool {
    verify_socket(path).is_ok()
}

pub fn verify_socket(path: &Path) -> Result<(), RuntimeError> {
    let metadata = fs::metadata(path).map_err(|source| RuntimeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.permissions().mode() & 0o177 != 0 {
        return Err(RuntimeError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "daemon socket must have mode 0600",
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn verify_socket_rejects_group_permissions() {
        let socket =
            std::env::temp_dir().join(format!("voxline-socket-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        std::fs::write(&socket, b"").expect("socket file");
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660)).expect("chmod");
        assert!(verify_socket(&socket).is_err());
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).expect("chmod");
        assert!(verify_socket(&socket).is_ok());
        let _ = std::fs::remove_file(socket);
    }
}
