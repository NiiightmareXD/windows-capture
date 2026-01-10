use crate::{capture::*, frame::Frame, graphics_capture_api::InternalCaptureControl, settings::*};
use smart_default::SmartDefault;
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

pub trait CaptureResult<E> {
    fn into_result(self) -> Result<(), E>;
    fn success() -> Self;
}

impl<E> CaptureResult<E> for Result<(), E> {
    fn into_result(self) -> Result<(), E> {
        self
    }
    fn success() -> Self {
        Ok(())
    }
}

impl CaptureResult<std::convert::Infallible> for () {
    fn into_result(self) -> Result<(), std::convert::Infallible> {
        Ok(())
    }
    fn success() -> Self {
        ()
    }
}
pub trait CaptureExt {
    fn start<F, R, E>(
        self,
        capture_settings: &CaptureSettings,
        on_frame_arrived: F,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(&mut Frame, InternalCaptureControl) -> R + Send + 'static,
        R: CaptureResult<E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static;

    fn start_with_closed_handler<F, G, R, E>(
        self,
        capture_settings: &CaptureSettings,
        on_frame_arrived: F,
        on_closed: G,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(&mut Frame, InternalCaptureControl) -> R + Send + 'static,
        G: FnMut() -> R + Send + 'static,
        R: CaptureResult<E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static;
}

impl<T> CaptureExt for T
where
    T: TryInto<GraphicsCaptureItemType>,
{
    fn start<F, R, E>(
        self,
        capture_settings: &CaptureSettings,
        on_frame_arrived: F,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(&mut Frame, InternalCaptureControl) -> R + Send + 'static,
        R: CaptureResult<E> + Send + 'static,
        E: Sync + Send + std::fmt::Debug + 'static,
    {
        self.start_with_closed_handler(capture_settings, on_frame_arrived, || R::success())
    }

    fn start_with_closed_handler<F, G, R, E>(
        self,
        capture_settings: &CaptureSettings,
        on_frame_arrived: F,
        on_closed: G,
    ) -> Result<(), GraphicsCaptureApiError<E>>
    where
        F: FnMut(&mut Frame, InternalCaptureControl) -> R + Send + 'static,
        G: FnMut() -> R + Send + 'static,
        R: CaptureResult<E> + Send + 'static,
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
            (on_frame_arrived, on_closed),
        );
        SimpleCapture::start(settings)
    }
}

struct SimpleCapture<F, G, R, E> {
    on_frame_arrived: F,
    on_closed: G,
    _phantom: std::marker::PhantomData<E>,
    _phantom2: std::marker::PhantomData<R>,
}

impl<F, G, R, E> GraphicsCaptureApiHandler for SimpleCapture<F, G, R, E>
where
    E: Sync + Send + std::fmt::Debug + 'static,
    F: FnMut(&mut Frame, InternalCaptureControl) -> R + Send + 'static,
    G: FnMut() -> R + Send + 'static,
    R: CaptureResult<E> + Send + 'static,
{
    type Flags = (F, G);
    type Error = E;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            on_frame_arrived: ctx.flags.0,
            on_closed: ctx.flags.1,
            _phantom: Default::default(),
            _phantom2: Default::default(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        (self.on_frame_arrived)(frame, capture_control).into_result()
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        (self.on_closed)().into_result()
    }
}
