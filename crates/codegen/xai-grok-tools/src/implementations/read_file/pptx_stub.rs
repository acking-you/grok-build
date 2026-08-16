//! PPTX API surface for builds that exclude Office document parsing.

pub(crate) fn extract_pptx_text_from_bytes(_bytes: &[u8]) -> Result<String, String> {
    Err("PPTX extraction is not included in the serve-runtime build".to_owned())
}
