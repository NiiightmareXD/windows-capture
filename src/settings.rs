use std::any::Any;

use windows::Graphics::Capture::GraphicsCaptureItem;

use crate::window::Window;

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum ColorFormat {
    Rgba16F = 10,
    Rgba8 = 28,
    Bgra8 = 87,
}

impl Default for ColorFormat {
    #[inline]
    fn default() -> Self {
        Self::Rgba8
    }
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum CursorCaptureSettings {
    Default,
    WithCursor,
    WithoutCursor,
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum DrawBorderSettings {
    Default,
    WithBorder,
    WithoutBorder,
}

#[derive(Eq, PartialEq, Clone, Debug)]
/// Represents the settings for screen capturing.
pub struct Settings<Flags, T: TryInto<GraphicsCaptureItem>> {
    /// The graphics capture item to capture.
    pub(crate) item: T,
    /// Specifies whether to capture the cursor.
    pub(crate) cursor_capture: CursorCaptureSettings,
    /// Specifies whether to draw a border around the captured region.
    pub(crate) draw_border: DrawBorderSettings,
    /// The color format for the captured graphics.
    pub(crate) color_format: ColorFormat,
    /// Additional flags for capturing graphics.
    pub(crate) flags: Flags,
    /// Specifies whether to exclude the window's title bar from the capture.
    ///
    /// If set to `true`, the capture will attempt to crop out the title bar.
    /// This calculation relies on the system's standard caption height metric (`SM_CYCAPTION`).
    pub(crate) exclude_title_bar: bool,
}

impl<Flags, T> Settings<Flags, T>
where
    T: TryInto<GraphicsCaptureItem> + Any + 'static,
{
    /// Create Capture Settings
    ///
    /// # Arguments
    ///
    /// * `item` - The graphics capture item.
    /// * `capture_cursor` - Whether to capture the cursor or not.
    /// * `draw_border` - Whether to draw a border around the captured region or not.
    /// * `color_format` - The desired color format for the captured frame.
    /// * `flags` - Additional flags for the capture settings that will be passed to user defined `new` function.
    /// * `exclude_title_bar` - Whether to attempt to exclude the window's title bar.
    #[must_use]
    #[inline]
    pub const fn new(
        item: T,
        cursor_capture: CursorCaptureSettings,
        draw_border: DrawBorderSettings,
        color_format: ColorFormat,
        flags: Flags,
        exclude_title_bar: bool,
    ) -> Self {
        Self {
            item,
            cursor_capture,
            draw_border,
            color_format,
            flags,
            exclude_title_bar,
        }
    }

    /// Get the item
    ///
    /// # Returns
    ///
    /// The item to be captured
    #[must_use]
    #[inline]
    pub const fn item(&self) -> &T {
        &self.item
    }

    /// Get the cursor capture settings
    ///
    /// # Returns
    ///
    /// The cursor capture settings
    #[must_use]
    #[inline]
    pub const fn cursor_capture(&self) -> CursorCaptureSettings {
        self.cursor_capture
    }

    /// Get the draw border settings
    ///
    /// # Returns
    ///
    /// The draw border settings
    #[must_use]
    #[inline]
    pub const fn draw_border(&self) -> DrawBorderSettings {
        self.draw_border
    }

    /// Get the color format
    ///
    /// # Returns
    ///
    /// The color format
    #[must_use]
    #[inline]
    pub const fn color_format(&self) -> ColorFormat {
        self.color_format
    }

    /// Get the flags
    ///
    /// # Returns
    ///
    /// The flags
    #[must_use]
    #[inline]
    pub const fn flags(&self) -> &Flags {
        &self.flags
    }

    /// Get the exclude title bar setting
    ///
    /// # Returns
    ///
    /// True if title bar exclusion is enabled, false otherwise.
    #[must_use]
    #[inline]
    pub const fn exclude_title_bar(&self) -> bool {
        self.exclude_title_bar
    }
}
pub trait AsWindow {
    fn as_window(&self) -> Option<&Window>;
}

impl<T: Any> AsWindow for T {
    fn as_window(&self) -> Option<&Window> {
        (self as &dyn Any).downcast_ref::<Window>()
    }
}
