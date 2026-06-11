use std::path::Path;

use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncReadExt,
    io::AsyncWriteExt,
};

use crate::models::{ModelCatalogEntry, download_url};

fn digest_hex(digest: impl AsRef<[u8]>) -> String {
    let mut hex = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(digest_hex(hasher.finalize()))
}

async fn verify_file_checksum(path: &Path, expected: &str) -> Result<(), DownloadError> {
    let actual = hash_file(path).await?;
    if actual == expected {
        return Ok(());
    }
    let _ = fs::remove_file(path).await;
    Err(DownloadError::ChecksumMismatch {
        expected: expected.to_owned(),
        actual,
    })
}

async fn remove_part(part_path: &Path) {
    let _ = fs::remove_file(part_path).await;
}

pub async fn download_model(
    entry: &ModelCatalogEntry,
    destination: &Path,
    progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<(), DownloadError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await?;
    }

    let part_path = destination.with_extension("part");

    if destination.is_file() {
        match verify_file_checksum(destination, entry.sha256).await {
            Ok(()) => return Ok(()),
            Err(DownloadError::ChecksumMismatch { .. }) => {}
            Err(err) => return Err(err),
        }
    }

    let url = download_url(entry);
    let client = Client::builder().user_agent("voxline-setup").build()?;
    let mut response = client.get(&url).send().await?.error_for_status()?;
    let total = response.content_length();

    let mut file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&part_path)
        .await
    {
        Ok(file) => file,
        Err(err) => return Err(err.into()),
    };

    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let download_result: Result<(), DownloadError> = async {
        while let Some(chunk) = response.chunk().await? {
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            if let Some(progress) = progress {
                progress(downloaded, total);
            }
        }
        file.flush().await?;
        Ok(())
    }
    .await;

    drop(file);

    if let Err(err) = download_result {
        remove_part(&part_path).await;
        return Err(err);
    }

    let actual = digest_hex(hasher.finalize());
    if actual != entry.sha256 {
        remove_part(&part_path).await;
        return Err(DownloadError::ChecksumMismatch {
            expected: entry.sha256.to_owned(),
            actual,
        });
    }

    if let Err(err) = fs::rename(&part_path, destination).await {
        remove_part(&part_path).await;
        return Err(err.into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha256_hex(bytes: &[u8]) -> String {
        digest_hex(Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn hash_file_matches_known_digest() {
        let path = std::env::temp_dir().join(format!("voxline-hash-test-{}", std::process::id()));
        let content = b"voxline checksum test payload";
        fs::write(&path, content).await.expect("write temp file");
        let expected = sha256_hex(content);
        let actual = hash_file(&path).await.expect("hash file");
        assert_eq!(actual, expected);
        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn checksum_mismatch_deletes_destination() {
        let path =
            std::env::temp_dir().join(format!("voxline-checksum-mismatch-{}", std::process::id()));
        let content = b"corrupted model bytes";
        fs::write(&path, content).await.expect("write temp file");
        let expected = sha256_hex(b"authoritative model bytes");
        let result = verify_file_checksum(&path, &expected).await;
        assert!(matches!(
            result,
            Err(DownloadError::ChecksumMismatch { .. })
        ));
        assert!(!path.exists());
    }
}
