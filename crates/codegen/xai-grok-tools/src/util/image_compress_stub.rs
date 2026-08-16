//! Image-compression API surface for builds without raster codecs.

#[derive(Debug, Clone, Copy)]
pub enum FilterType {
    CatmullRom,
    Lanczos3,
}

#[derive(Debug, Clone, Copy)]
pub struct ReEncodeParams {
    pub max_bytes: usize,
    pub max_side_px: u32,
    pub max_pixels: u64,
    pub min_side_px: u32,
    pub quality_steps: &'static [u8],
    pub filter: FilterType,
}

impl ReEncodeParams {
    pub fn exceeds_dimension_caps(&self, w: u32, h: u32) -> bool {
        w > self.max_side_px
            || h > self.max_side_px
            || u64::from(w) * u64::from(h) > self.max_pixels
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ReEncodeError {
    #[error(
        "re-encode could not fit under {max_bytes} bytes after PNG+JPEG attempts (last side {last_side}px)"
    )]
    CouldNotFit { max_bytes: usize, last_side: u32 },
}
