//! Read-file image API surface for builds without raster codecs.

#[derive(Debug, thiserror::Error)]
pub enum CompressImageError {
    #[error("image decoding is unavailable in this serve-runtime build")]
    DecodeUnavailable,
}

pub const MAX_IMAGE_PAYLOAD_BYTES: usize = 768 * 1024;

pub fn compress_image_for_conversation(
    _raw_bytes: Vec<u8>,
    _original_mime: String,
) -> Result<(Vec<u8>, String), CompressImageError> {
    Err(CompressImageError::DecodeUnavailable)
}

pub async fn image_read_output(
    _file_bytes: Vec<u8>,
    _mime_type: String,
) -> crate::types::output::ReadFileOutput {
    crate::types::output::ReadFileOutput::ImageSizeError(
        "Image reading is unavailable in this serve-runtime build.".to_owned(),
    )
}
