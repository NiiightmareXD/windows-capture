use crate::{capture::*, frame::Frame, graphics_capture_api::InternalCaptureControl, settings::*};
use smart_default::SmartDefault;
/// Represents the settings for a capture session.
#[derive(SmartDefault, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureSettings {
    /// Specifies whether the cursor should be captured.
    #[default(CursorCaptureSettings::Default)]
    pub cursor_capture_settings: CursorCaptureSettings,
    /// Specifies whether a border should be drawn around the captured item.
    #[default(DrawBorderSettings::Default)]
    pub draw_border_settings: DrawBorderSettings,
    /// Specifies whether to include secondary windows in the capture.
    #[default(SecondaryWindowSettings::Default)]
    pub secondary_window_settings: SecondaryWindowSettings,
    /// Specifies the minimum time between frame updates.
    #[default(MinimumUpdateIntervalSettings::Default)]
    pub minimum_update_interval_settings: MinimumUpdateIntervalSettings,
    /// Specifies how to handle dirty regions.
    #[default(DirtyRegionSettings::Default)]
    pub dirty_region_settings: DirtyRegionSettings,
    /// Specifies the pixel format for the captured frames.
    #[default(ColorFormat::Rgba8)]
    pub color_format: ColorFormat,
}
/// A trait that provides extension methods for starting a capture session.
pub trait CaptureExt {
    /// Starts a capture session.
    ///
    /// This function **blocks the current thread** until the capture session is finished.
    ///
    /// # Arguments
    ///
    /// * `capture_settings` - The settings for the capture session.
    /// * `handler` - A closure that handles capture events. It receives an `Option<(&mut Frame, InternalCaptureControl)>`.
    ///   - `Some((frame, control))` when a new frame is available.
    ///   - `None` when the captured item (e.g., window or monitor) is closed before `control.stop()` is called.
    ///
    /// # Behavior
    ///
    /// - **Blocking**: This function will block the caller's thread until the capture ends.
    /// - **Error Propagation**: If the `handler` returns an `Err`, it will be propagated and the function will return it.
    /// - **Closure**: The `handler` is called with `None` only if the captured item is closed externally. Calling `control.stop()` terminates the capture session immediately without an additional call to the handler with `None`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use windows_capture::capture_ext::{CaptureExt, CaptureSettings};
    /// use windows_capture::encoder::ImageFormat;
    /// use windows_capture::window::Window;
    ///
    /// let item = Window::foreground().unwrap();
    /// let image_path = "target/frame.png";
    ///
    /// item.start(CaptureSettings::default(), move |event| {
    ///     if let Some((frame, control)) = event {
    ///         frame.save_as_image(image_path, ImageFormat::Png).unwrap();
    ///         println!("Saved frame to {}", image_path);
    ///         control.stop();
    ///     } else {
    ///         eprintln!("Capture failed: Item closed");
    ///     }
    ///     Ok::<(), ()>(())
    /// }).expect("Failed to start capture");
    /// ```
    fn start<F, E>(self, capture_settings: CaptureSettings, handler: F) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(Option<(&mut Frame, InternalCaptureControl)>) -> Result<(), E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static;

    /// Starts a capture session with the specified settings and handler in a free-threaded manner.
    fn start_free_threaded<F, E>(
        self,
        capture_settings: CaptureSettings,
        handler: F,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(Option<(&mut Frame, InternalCaptureControl)>) -> Result<(), E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static;
}

impl<T> CaptureExt for T
where
    T: TryInto<GraphicsCaptureItemType>,
{
    fn start<F, E>(self, capture_settings: CaptureSettings, handler: F) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(Option<(&mut Frame, InternalCaptureControl)>) -> Result<(), E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static,
    {
        let settings = Settings::new(
            self,
            capture_settings.cursor_capture_settings,
            capture_settings.draw_border_settings,
            capture_settings.secondary_window_settings,
            capture_settings.minimum_update_interval_settings,
            capture_settings.dirty_region_settings,
            capture_settings.color_format,
            handler,
        );
        SimpleCapture::start(settings)
    }

    fn start_free_threaded<F, E>(
        self,
        _capture_settings: CaptureSettings,
        _on_frame_arrived: F,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(Option<(&mut Frame, InternalCaptureControl)>) -> Result<(), E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static,
    {
        unimplemented!(
            "This is omitted for now because the underlying internal API (Capture::start_free_threaded(settings)) is currently causing compilation errors and requires further investigation."
        )
    }
}

struct SimpleCapture<F, E> {
    handler: F,
    _phantom: std::marker::PhantomData<E>,
}

impl<F, E> GraphicsCaptureApiHandler for SimpleCapture<F, E>
where
    E: Sync + Send + std::fmt::Debug + 'static,
    F: FnMut(Option<(&mut Frame, InternalCaptureControl)>) -> Result<(), E> + Send + 'static,
{
    type Flags = F;
    type Error = E;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { handler: ctx.flags, _phantom: Default::default() })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        (self.handler)(Some((frame, capture_control)))
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        (self.handler)(None)
    }
}
