use std::path::Path;

use reqwest::Client;
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};

use crate::models::{ModelCatalogEntry, download_url};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("download incomplete: expected at least {expected} bytes, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
}

pub async fn download_model(
    entry: &ModelCatalogEntry,
    destination: &Path,
    progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<(), DownloadError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await?;
    }
    if destination.is_file() {
        let metadata = fs::metadata(destination).await?;
        let min_bytes = entry
            .approx_size_mib
            .saturating_mul(1024)
            .saturating_mul(1024)
            / 2;
        if metadata.len() >= min_bytes {
            return Ok(());
        }
    }

    let part_path = destination.with_extension("part");
    let url = download_url(entry);
    let client = Client::builder().user_agent("voxline-setup").build()?;
    let mut response = client.get(&url).send().await?.error_for_status()?;
    let total = response.content_length();

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&part_path)
        .await?;

    let mut downloaded = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if let Some(progress) = progress {
            progress(downloaded, total);
        }
    }
    file.flush().await?;
    drop(file);
    fs::rename(&part_path, destination).await?;

    let actual = fs::metadata(destination).await?.len();
    let min_bytes = entry
        .approx_size_mib
        .saturating_mul(1024)
        .saturating_mul(1024)
        / 4;
    if actual < min_bytes {
        return Err(DownloadError::SizeMismatch {
            expected: min_bytes,
            actual,
        });
    }
    Ok(())
}
