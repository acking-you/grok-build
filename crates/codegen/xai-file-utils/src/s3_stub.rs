//! S3 API surface for builds that exclude native cloud-storage clients.

use std::path::Path;
use std::time::Duration;

/// Static access-key credentials for presigning S3 URLs.
#[derive(Clone)]
pub struct S3StaticCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for S3StaticCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3StaticCredentials")
            .field("access_key_id", &"[redacted]")
            .field("secret_access_key", &"[redacted]")
            .finish()
    }
}

fn unavailable<T>() -> anyhow::Result<T> {
    anyhow::bail!("native S3 support is not included in this build")
}

pub async fn presign_put_url(
    _region: &str,
    _endpoint_url: Option<&str>,
    _creds: &S3StaticCredentials,
    _bucket: &str,
    _key: &str,
    _content_type: &str,
    _expires_in: Duration,
) -> anyhow::Result<String> {
    unavailable()
}

pub async fn presign_get_url(
    _region: &str,
    _endpoint_url: Option<&str>,
    _creds: &S3StaticCredentials,
    _bucket: &str,
    _key: &str,
    _expires_in: Duration,
) -> anyhow::Result<String> {
    unavailable()
}

pub async fn upload_bytes(
    _bucket: &str,
    _object_path: &str,
    _content: &[u8],
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    unavailable()
}

pub async fn upload_file(
    _bucket: &str,
    _object_path: &str,
    _file_path: &Path,
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    unavailable()
}

pub async fn upload_stream<R: tokio::io::AsyncRead + Send + Sync + 'static>(
    _bucket: &str,
    _object_path: &str,
    _reader: R,
    _content_type: &str,
    _region: &str,
    _credentials_content: Option<&str>,
    _credentials_file: Option<&str>,
    _endpoint_url: Option<&str>,
) -> anyhow::Result<String> {
    unavailable()
}
