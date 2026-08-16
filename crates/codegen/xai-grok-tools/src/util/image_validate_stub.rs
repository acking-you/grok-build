//! Image-validation API surface for builds without raster codecs.

#[derive(Debug, thiserror::Error)]
pub enum ImageValidateError {
    #[error("image decoding is unavailable in this serve-runtime build")]
    Unsupported,
}

pub fn validate_image_bytes_with(
    _bytes: &[u8],
    _validate_full_decode: bool,
) -> Result<(u32, u32, &'static str), ImageValidateError> {
    Err(ImageValidateError::Unsupported)
}

pub fn validate_image_bytes(_bytes: &[u8]) -> Result<(u32, u32, &'static str), ImageValidateError> {
    Err(ImageValidateError::Unsupported)
}

pub fn jpeg_reaches_eoi(_bytes: &[u8]) -> bool {
    false
}

pub fn png_structurally_valid(_bytes: &[u8]) -> bool {
    false
}

pub fn webp_riff_complete(_bytes: &[u8]) -> bool {
    false
}

pub fn image_structurally_complete(_bytes: &[u8]) -> bool {
    false
}

pub fn needs_endpoint_transcode(_bytes: &[u8]) -> bool {
    false
}

pub fn transcode_to_endpoint_png(_bytes: &[u8]) -> Option<Result<Vec<u8>, ImageValidateError>> {
    None
}
