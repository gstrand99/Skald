use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("XDG_RUNTIME_DIR is unavailable; configure an alternate runtime directory explicitly")]
    MissingXdgRuntimeDir,
    #[error("runtime path is not owned by the current user: {0}")]
    NotOwned(PathBuf),
    #[error("runtime directory operation failed for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub fn runtime_dir() -> Result<PathBuf, RuntimeError> {
    let base = env::var_os("XDG_RUNTIME_DIR").ok_or(RuntimeError::MissingXdgRuntimeDir)?;
    Ok(PathBuf::from(base).join("voxline"))
}

pub fn socket_path() -> Result<PathBuf, RuntimeError> {
    Ok(runtime_dir()?.join("voxlined.sock"))
}

pub fn ensure_runtime_dir() -> Result<PathBuf, RuntimeError> {
    let path = runtime_dir()?;
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
