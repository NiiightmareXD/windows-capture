use windows::Graphics::Capture::GraphicsCaptureItem;

/// Used To Handle Settings Errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed To Convert To GraphicsCaptureItem")]
    ItemConvertFailed,
}

/// Kind Of Pixel Format For Frame To Have
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum ColorFormat {
    Rgba8,
    Bgra8,
    NV12,
}

/// Capture Settings, None Means Default
#[derive(Eq, PartialEq, Clone, Debug)]
pub struct Settings<Flags> {
    pub item: GraphicsCaptureItem,
    pub capture_cursor: Option<bool>,
    pub draw_border: Option<bool>,
    pub color_format: ColorFormat,
    pub flags: Flags,
}

impl<Flags> Settings<Flags> {
    /// Create Capture Settings
    pub fn new<T: TryInto<GraphicsCaptureItem>>(
        item: T,
        capture_cursor: Option<bool>,
        draw_border: Option<bool>,
        color_format: ColorFormat,
        flags: Flags,
    ) -> Result<Self, Error> {
        Ok(Self {
            item: match item.try_into() {
                Ok(item) => item,
                Err(_) => return Err(Error::ItemConvertFailed),
            },
            capture_cursor,
            draw_border,
            color_format,
            flags,
        })
    }
}
