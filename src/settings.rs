use thiserror::Error;
use windows::Graphics::Capture::GraphicsCaptureItem;

/// Used To Handle Internal Settings Errors
#[derive(Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum SettingsErrors {
    #[error("Failed To Convert To GraphicsCaptureItem")]
    ConvertFailed,
}

/// Capture Settings, None Means Default
#[derive(Eq, PartialEq, Clone, Debug)]
pub struct WindowsCaptureSettings<Flags> {
    pub item: GraphicsCaptureItem,
    pub capture_cursor: Option<bool>,
    pub draw_border: Option<bool>,
    pub flags: Flags,
}

impl<Flags> WindowsCaptureSettings<Flags> {
    /// Create Capture Settings
    pub fn new<T: TryInto<GraphicsCaptureItem>>(
        item: T,
        capture_cursor: Option<bool>,
        draw_border: Option<bool>,
        flags: Flags,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            item: match item.try_into() {
                Ok(item) => item,
                Err(_) => return Err(Box::new(SettingsErrors::ConvertFailed)),
            },
            capture_cursor,
            draw_border,
            flags,
        })
    }
}
