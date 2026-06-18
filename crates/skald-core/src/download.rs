use std::{path::Path, time::Duration};

use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncReadExt,
    io::AsyncWriteExt,
};

use crate::{
    models::{ModelCatalogEntry, download_url},
    system_probe::free_space_mib,
};

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
    #[error("size mismatch: expected {expected} bytes, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("refusing non-HTTPS model URL: {0}")]
    InsecureUrl(String),
    #[error("destination already contains an unrelated file: {0}")]
    DestinationExists(String),
    #[error("download failed after {attempts} attempts: {message}")]
    RetriesExhausted { attempts: u8, message: String },
    #[error("insufficient disk space: need {needed_mib} MiB, have {available_mib} MiB")]
    InsufficientSpace { needed_mib: u64, available_mib: u64 },
}

struct PartialDownload {
    path: std::path::PathBuf,
    placed: bool,
}

impl PartialDownload {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            placed: false,
        }
    }
}

impl Drop for PartialDownload {
    fn drop(&mut self) {
        if !self.placed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
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

pub async fn verify_model_file(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), DownloadError> {
    let actual_size = fs::metadata(path).await?.len();
    if actual_size != expected_size {
        return Err(DownloadError::SizeMismatch {
            expected: expected_size,
            actual: actual_size,
        });
    }
    let actual = hash_file(path).await?;
    if actual == expected_sha256 {
        return Ok(());
    }
    Err(DownloadError::ChecksumMismatch {
        expected: expected_sha256.to_owned(),
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
    let url = download_url(entry);
    download_model_from_url(entry, destination, &url, progress, true).await
}

async fn download_model_from_url(
    entry: &ModelCatalogEntry,
    destination: &Path,
    url: &str,
    progress: Option<&dyn Fn(u64, Option<u64>)>,
    require_https: bool,
) -> Result<(), DownloadError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).await?;
        if let Some(available_mib) = free_space_mib(parent) {
            let needed_mib = entry.expected_size.div_ceil(1024 * 1024);
            if available_mib < needed_mib {
                return Err(DownloadError::InsufficientSpace {
                    needed_mib,
                    available_mib,
                });
            }
        }
    }

    let part_path = destination.with_extension(format!("{}.part", ulid::Ulid::new()));

    if destination.is_file() {
        match verify_model_file(destination, entry.expected_size, entry.sha256).await {
            Ok(()) => return Ok(()),
            Err(_) => {
                return Err(DownloadError::DestinationExists(
                    destination.display().to_string(),
                ));
            }
        }
    }

    if require_https && !url.starts_with("https://") {
        return Err(DownloadError::InsecureUrl(url.to_owned()));
    }
    let client = Client::builder().user_agent("skald-models").build()?;
    let attempts = 3_u8;
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        remove_part(&part_path).await;
        match download_once(&client, entry, destination, &part_path, url, progress).await {
            Ok(()) => return Ok(()),
            Err(
                error @ (DownloadError::ChecksumMismatch { .. }
                | DownloadError::SizeMismatch { .. }
                | DownloadError::DestinationExists(_)
                | DownloadError::InsecureUrl(_)
                | DownloadError::InsufficientSpace { .. }),
            ) => return Err(error),
            Err(error) => {
                last_error = error.to_string();
                remove_part(&part_path).await;
                if attempt < attempts {
                    tokio::time::sleep(Duration::from_millis(100 * u64::from(attempt))).await;
                }
            }
        }
    }
    Err(DownloadError::RetriesExhausted {
        attempts,
        message: last_error,
    })
}

async fn download_once(
    client: &Client,
    entry: &ModelCatalogEntry,
    destination: &Path,
    part_path: &Path,
    url: &str,
    progress: Option<&dyn Fn(u64, Option<u64>)>,
) -> Result<(), DownloadError> {
    let mut partial = PartialDownload::new(part_path);
    let mut response = client.get(url).send().await?.error_for_status()?;
    let total = response.content_length();
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(part_path)
        .await?;

    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if let Some(progress) = progress {
            progress(downloaded, total);
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    let actual = digest_hex(hasher.finalize());
    if downloaded != entry.expected_size {
        return Err(DownloadError::SizeMismatch {
            expected: entry.expected_size,
            actual: downloaded,
        });
    }
    if actual != entry.sha256 {
        return Err(DownloadError::ChecksumMismatch {
            expected: entry.sha256.to_owned(),
            actual,
        });
    }

    match fs::hard_link(part_path, destination).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(DownloadError::DestinationExists(
                destination.display().to_string(),
            ));
        }
        Err(error) => return Err(error.into()),
    }
    fs::remove_file(part_path).await?;
    partial.placed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn sha256_hex(bytes: &[u8]) -> String {
        digest_hex(Sha256::digest(bytes))
    }

    #[tokio::test]
    async fn hash_file_matches_known_digest() {
        let path = std::env::temp_dir().join(format!("skald-hash-test-{}", std::process::id()));
        let content = b"skald checksum test payload";
        fs::write(&path, content).await.expect("write temp file");
        let expected = sha256_hex(content);
        let actual = hash_file(&path).await.expect("hash file");
        assert_eq!(actual, expected);
        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn checksum_mismatch_is_reported_without_deleting_destination() {
        let path =
            std::env::temp_dir().join(format!("skald-checksum-mismatch-{}", std::process::id()));
        let content = b"corrupted model bytes";
        fs::write(&path, content).await.expect("write temp file");
        let expected = sha256_hex(b"authoritative model bytes");
        let result = verify_model_file(&path, content.len() as u64, &expected).await;
        assert!(matches!(
            result,
            Err(DownloadError::ChecksumMismatch { .. })
        ));
        assert!(path.exists());
        let _ = fs::remove_file(path).await;
    }

    async fn fixture_url(body: &'static [u8], declared_length: usize) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
        });
        format!("http://{address}/model.bin")
    }

    fn fixture_entry(body: &[u8]) -> ModelCatalogEntry {
        let sha256 = Box::leak(sha256_hex(body).into_boxed_str());
        ModelCatalogEntry {
            id: "fixture",
            file_name: "fixture.bin",
            gpu: false,
            expected_size: body.len() as u64,
            approx_size_mib: 1,
            sha256,
            language: "test",
            intended_use: "test",
            hardware_guidance: "test",
            description: "test",
        }
    }

    #[tokio::test]
    async fn fixture_download_is_verified_and_atomically_placed() {
        let body = b"fixture model bytes";
        let entry = fixture_entry(body);
        let directory = std::env::temp_dir().join(format!("skald-download-{}", ulid::Ulid::new()));
        let destination = directory.join(entry.file_name);
        let url = fixture_url(body, body.len()).await;
        download_model_from_url(&entry, &destination, &url, None, false)
            .await
            .unwrap();
        assert_eq!(fs::read(&destination).await.unwrap(), body);
        assert!(!destination.with_extension("part").exists());
        let _ = fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn failed_fixture_download_removes_partial_file() {
        let body = b"partial";
        let entry = fixture_entry(b"complete model");
        let directory = std::env::temp_dir().join(format!("skald-partial-{}", ulid::Ulid::new()));
        let destination = directory.join(entry.file_name);
        let url = fixture_url(body, body.len()).await;
        let error = download_model_from_url(&entry, &destination, &url, None, false)
            .await
            .unwrap_err();
        assert!(matches!(error, DownloadError::SizeMismatch { .. }));
        assert!(!destination.exists());
        assert!(!destination.with_extension("part").exists());
        let _ = fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn checksum_mismatch_from_fixture_removes_partial_file() {
        let body = b"wrong checksum bytes";
        let mut entry = fixture_entry(body);
        entry.sha256 = Box::leak(sha256_hex(b"different same size").into_boxed_str());
        let directory = std::env::temp_dir().join(format!("skald-checksum-{}", ulid::Ulid::new()));
        let destination = directory.join(entry.file_name);
        let url = fixture_url(body, body.len()).await;
        let error = download_model_from_url(&entry, &destination, &url, None, false)
            .await
            .unwrap_err();
        assert!(matches!(error, DownloadError::ChecksumMismatch { .. }));
        assert!(!destination.exists());
        assert!(!destination.with_extension("part").exists());
        let _ = fs::remove_dir_all(directory).await;
    }

    #[tokio::test]
    async fn unrelated_destination_is_never_overwritten() {
        let body = b"fixture model bytes";
        let entry = fixture_entry(body);
        let directory = std::env::temp_dir().join(format!("skald-existing-{}", ulid::Ulid::new()));
        fs::create_dir_all(&directory).await.unwrap();
        let destination = directory.join(entry.file_name);
        fs::write(&destination, b"user data").await.unwrap();
        let url = fixture_url(body, body.len()).await;
        let error = download_model_from_url(&entry, &destination, &url, None, false)
            .await
            .unwrap_err();
        assert!(matches!(error, DownloadError::DestinationExists(_)));
        assert_eq!(fs::read(&destination).await.unwrap(), b"user data");
        let _ = fs::remove_dir_all(directory).await;
    }
}
