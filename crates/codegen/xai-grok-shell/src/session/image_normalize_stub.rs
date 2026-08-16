//! Attachment-normalization API for builds without raster codecs.

use agent_client_protocol::ImageContent;

use crate::session::normalize_cache::NormalizeCache;

pub(crate) const MAX_IMAGE_BYTES: usize = 1_500_000;
pub(crate) const MIN_VISION_SIDE_PX: u32 = 8;
pub(crate) const MIN_VISION_TOTAL_PX: u64 = 512;
pub(crate) const MAX_VISION_TOTAL_PX: u64 = 178_956_970;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageCompressionInfo {
    pub index: usize,
    pub original_bytes: usize,
    pub compressed_bytes: usize,
    pub original_width: u32,
    pub original_height: u32,
    pub compressed_width: u32,
    pub compressed_height: u32,
    pub exceeded_size: bool,
    pub exceeded_dimensions: bool,
}

impl ImageCompressionInfo {
    pub(crate) fn display(&self) -> String {
        format!("Image {} compression unavailable", self.index)
    }
}

#[derive(Default)]
pub(crate) struct NormalizeResult {
    pub images: Vec<ImageContent>,
    pub compressed: Vec<ImageCompressionInfo>,
    pub re_encode_fallbacks: Vec<String>,
    pub dropped: Vec<String>,
}

pub(crate) async fn normalize_images(
    images: Vec<ImageContent>,
    is_cursor: bool,
) -> NormalizeResult {
    normalize_images_in(images, is_cursor, NormalizeCache::global()).await
}

pub(crate) async fn normalize_images_in(
    images: Vec<ImageContent>,
    _is_cursor: bool,
    _cache: &NormalizeCache,
) -> NormalizeResult {
    let dropped = (1..=images.len())
        .map(|index| {
            format!(
                "Image {index} was dropped before send: image decoding is unavailable in this serve-runtime build."
            )
        })
        .collect();
    NormalizeResult {
        dropped,
        ..NormalizeResult::default()
    }
}

fn render_notice(notes: &[String], inner_tag: &str) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let tag = xai_grok_tools::reminders::DEFAULT_REMINDER_TAG;
    format!(
        "\n\n<{tag}>\n<{inner_tag}>\n{}\n</{inner_tag}>\n</{tag}>",
        notes.join("\n"),
    )
}

pub(crate) fn render_image_dropped_notice(notes: &[String], _is_cursor: bool) -> String {
    render_notice(notes, "image_dropped_notice")
}

pub(crate) fn dropped_to_envelope(
    dropped: Vec<String>,
    is_cursor: bool,
) -> Option<(String, Vec<String>)> {
    if dropped.is_empty() {
        return None;
    }
    Some((render_image_dropped_notice(&dropped, is_cursor), dropped))
}

pub(crate) fn render_re_encode_fallback_notice(notes: &[String], _is_cursor: bool) -> String {
    render_notice(notes, "image_re_encode_fallback")
}

pub(crate) fn render_compression_notice(
    compressed: &[ImageCompressionInfo],
    _is_cursor: bool,
) -> String {
    let notes: Vec<_> = compressed
        .iter()
        .map(ImageCompressionInfo::display)
        .collect();
    render_notice(&notes, "image_compression_notice")
}

pub(crate) fn persisted_image_reject_reason(bytes: &[u8]) -> Option<String> {
    Some(format!(
        "image decoding unavailable in serve-runtime build ({} bytes)",
        bytes.len()
    ))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InlineAttachVerdict {
    Attach,
    TooSmall,
    Unreadable,
}

pub(crate) fn inline_attach_verdict(_data_b64: &str) -> InlineAttachVerdict {
    InlineAttachVerdict::Unreadable
}
