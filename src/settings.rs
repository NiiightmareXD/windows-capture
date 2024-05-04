use windows::Graphics::Capture::GraphicsCaptureItem;

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum ColorFormat {
    Rgba16F = 10,
    Rgba8 = 28,
    Bgra8 = 87,
}

impl Default for ColorFormat {
    fn default() -> Self {
        Self::Rgba8
    }
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub enum CursorCaptureSettings {
    Default,
    WithCursor,
    WithoutCursor,
}

#[derive(Eq, PartialEq, Clone, Debug)]
pub enum DrawBorderSettings {
    Default,
    WithBorder,
    WithoutBorder,
}

#[derive(Eq, PartialEq, Clone, Debug)]
/// Represents the settings for screen capturing.
pub struct Settings<Flags, T: TryInto<GraphicsCaptureItem>> {
    /// The graphics capture item to capture.
    pub item: T,
    /// Specifies whether to capture the cursor.
    pub cursor_capture: CursorCaptureSettings,
    /// Specifies whether to draw a border around the captured region.
    pub draw_border: DrawBorderSettings,
    /// The color format for the captured graphics.
    pub color_format: ColorFormat,
    /// Additional flags for capturing graphics.
    pub flags: Flags,
}

impl<Flags, T: TryInto<GraphicsCaptureItem>> Settings<Flags, T> {
    /// Create Capture Settings
    ///
    /// # Arguments
    ///
    /// * `item` - The graphics capture item.
    /// * `capture_cursor` - Whether to capture the cursor or not.
    /// * `draw_border` - Whether to draw a border around the captured region or not.
    /// * `color_format` - The desired color format for the captured frame.
    /// * `flags` - Additional flags for the capture settings that will be passed to user defined `new` function.
    pub const fn new(
        item: T,
        cursor_capture: CursorCaptureSettings,
        draw_border: DrawBorderSettings,
        color_format: ColorFormat,
        flags: Flags,
    ) -> Self {
        Self {
            item,
            cursor_capture,
            draw_border,
            color_format,
            flags,
        }
    }
}
