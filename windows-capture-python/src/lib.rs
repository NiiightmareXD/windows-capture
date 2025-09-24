#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::multiple_crate_versions)] // Should update as soon as possible

use std::sync::Arc;
use std::time::Duration;

use ::windows_capture::capture::{
    CaptureControl, CaptureControlError, Context, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use ::windows_capture::frame::{self, Frame};
use ::windows_capture::graphics_capture_api::InternalCaptureControl;
use ::windows_capture::monitor::Monitor;
use ::windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};
use ::windows_capture::window::Window;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyList;

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeWindowsCapture>()?;
    m.add_class::<NativeCaptureControl>()?;
    Ok(())
}

/// Internal struct used to handle free threaded start.
#[pyclass]
pub struct NativeCaptureControl {
    capture_control: Option<CaptureControl<InnerNativeWindowsCapture, InnerNativeWindowsCaptureError>>,
}

impl NativeCaptureControl {
    #[inline]
    #[must_use]
    const fn new(capture_control: CaptureControl<InnerNativeWindowsCapture, InnerNativeWindowsCaptureError>) -> Self {
        Self { capture_control: Some(capture_control) }
    }
}

#[pymethods]
impl NativeCaptureControl {
    #[inline]
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.capture_control.as_ref().is_none_or(CaptureControl::is_finished)
    }

    #[inline]
    pub fn wait(&mut self, py: Python) -> PyResult<()> {
        py.detach(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.wait() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::GraphicsCaptureApiError(
                            GraphicsCaptureApiError::FrameHandlerError(InnerNativeWindowsCaptureError::PythonError(
                                ref e,
                            )),
                        ) = e
                        {
                            return Err(PyException::new_err(format!("Failed to join the capture thread: {e}",)));
                        }

                        return Err(PyException::new_err(format!("Failed to join the capture thread: {e}",)));
                    }
                };
            }

            Ok(())
        })?;

        Ok(())
    }

    #[inline]
    pub fn stop(&mut self, py: Python) -> PyResult<()> {
        py.detach(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.stop() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::GraphicsCaptureApiError(
                            GraphicsCaptureApiError::FrameHandlerError(InnerNativeWindowsCaptureError::PythonError(
                                ref e,
                            )),
                        ) = e
                        {
                            return Err(PyException::new_err(format!("Failed to stop the capture thread: {e}",)));
                        }

                        return Err(PyException::new_err(format!("Failed to stop the capture thread: {e}",)));
                    }
                };
            }

            Ok(())
        })?;

        Ok(())
    }
}

/// Internal struct used for Windows capture.
#[pyclass]
pub struct NativeWindowsCapture {
    on_frame_arrived_callback: Arc<Py<PyAny>>,
    on_closed: Arc<Py<PyAny>>,
    cursor_capture: CursorCaptureSettings,
    draw_border: DrawBorderSettings,
    secondary_window: SecondaryWindowSettings,
    minimum_update_interval: MinimumUpdateIntervalSettings,
    dirty_region_settings: DirtyRegionSettings,
    monitor_index: Option<usize>,
    window_name: Option<String>,
}

#[pymethods]
impl NativeWindowsCapture {
    #[new]
    #[pyo3(signature = (on_frame_arrived_callback, on_closed, cursor_capture=None, draw_border=None, secondary_window=None, minimum_update_interval=None, dirty_region=None, monitor_index=None, window_name=None))]
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        on_frame_arrived_callback: Py<PyAny>,
        on_closed: Py<PyAny>,
        cursor_capture: Option<bool>,
        draw_border: Option<bool>,
        secondary_window: Option<bool>,
        minimum_update_interval: Option<u64>,
        dirty_region: Option<bool>,
        mut monitor_index: Option<usize>,
        window_name: Option<String>,
    ) -> PyResult<Self> {
        if window_name.is_some() && monitor_index.is_some() {
            return Err(PyException::new_err("You can't specify both the monitor index and the window name"));
        }

        if window_name.is_none() && monitor_index.is_none() {
            monitor_index = Some(1);
        }

        let cursor_capture = match cursor_capture {
            Some(true) => CursorCaptureSettings::WithCursor,
            Some(false) => CursorCaptureSettings::WithoutCursor,
            None => CursorCaptureSettings::Default,
        };

        let draw_border = match draw_border {
            Some(true) => DrawBorderSettings::WithBorder,
            Some(false) => DrawBorderSettings::WithoutBorder,
            None => DrawBorderSettings::Default,
        };

        let secondary_window = match secondary_window {
            Some(true) => SecondaryWindowSettings::Include,
            Some(false) => SecondaryWindowSettings::Exclude,
            None => SecondaryWindowSettings::Default,
        };

        let minimum_update_interval = minimum_update_interval
            .map_or(MinimumUpdateIntervalSettings::Default, |interval| {
                MinimumUpdateIntervalSettings::Custom(Duration::from_millis(interval))
            });

        let dirty_region_settings = match dirty_region {
            Some(true) => DirtyRegionSettings::ReportAndRender,
            Some(false) => DirtyRegionSettings::ReportOnly,
            None => DirtyRegionSettings::Default,
        };

        Ok(Self {
            on_frame_arrived_callback: Arc::new(on_frame_arrived_callback),
            on_closed: Arc::new(on_closed),
            cursor_capture,
            draw_border,
            secondary_window,
            minimum_update_interval,
            dirty_region_settings,
            monitor_index,
            window_name,
        })
    }

    /// Start capture.
    #[inline]
    pub fn start(&mut self) -> PyResult<()> {
        if self.window_name.is_some() {
            let window = match Window::from_contains_name(self.window_name.as_ref().unwrap()) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to find window: {e}")));
                }
            };

            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                (self.on_frame_arrived_callback.clone(), self.on_closed.clone()),
            );

            match InnerNativeWindowsCapture::start(settings) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start threw an exception: {e}",
                    )));
                }
            }
        } else {
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to get monitor from index: {e}")));
                }
            };

            let settings = Settings::new(
                monitor,
                self.cursor_capture,
                self.draw_border,
                self.secondary_window,
                self.minimum_update_interval,
                self.dirty_region_settings,
                ColorFormat::Bgra8,
                (self.on_frame_arrived_callback.clone(), self.on_closed.clone()),
            );

            match InnerNativeWindowsCapture::start(settings) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start threw an exception: {e}",
                    )));
                }
            }
        };

        Ok(())
    }

    /// Start capture on a dedicated thread.
    #[inline]
    pub fn start_free_threaded(&mut self) -> PyResult<NativeCaptureControl> {
        let capture_control = if self.window_name.is_some() {
            let window = match Window::from_contains_name(self.window_name.as_ref().unwrap()) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to find window: {e}")));
                }
            };

            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                (self.on_frame_arrived_callback.clone(), self.on_closed.clone()),
            );

            let capture_control = match InnerNativeWindowsCapture::start_free_threaded(settings) {
                Ok(capture_control) => capture_control,
                Err(e) => {
                    if let GraphicsCaptureApiError::FrameHandlerError(InnerNativeWindowsCaptureError::PythonError(
                        ref e,
                    )) = e
                    {
                        return Err(PyException::new_err(format!("Capture session threw an exception: {e}",)));
                    }

                    return Err(PyException::new_err(format!("Capture session threw an exception: {e}",)));
                }
            };

            NativeCaptureControl::new(capture_control)
        } else {
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to get monitor from index: {e}")));
                }
            };

            let settings = Settings::new(
                monitor,
                self.cursor_capture,
                self.draw_border,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Default,
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                (self.on_frame_arrived_callback.clone(), self.on_closed.clone()),
            );

            let capture_control = match InnerNativeWindowsCapture::start_free_threaded(settings) {
                Ok(capture_control) => capture_control,
                Err(e) => {
                    if let GraphicsCaptureApiError::FrameHandlerError(InnerNativeWindowsCaptureError::PythonError(
                        ref e,
                    )) = e
                    {
                        return Err(PyException::new_err(format!("Capture session threw an exception: {e}",)));
                    }

                    return Err(PyException::new_err(format!("Capture session threw an exception: {e}",)));
                }
            };

            NativeCaptureControl::new(capture_control)
        };

        Ok(capture_control)
    }
}

struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<Py<PyAny>>,
    on_closed: Arc<Py<PyAny>>,
}

#[derive(thiserror::Error, Debug)]
pub enum InnerNativeWindowsCaptureError {
    #[error("Python callback error: {0}")]
    PythonError(pyo3::PyErr),
    #[error("Frame process error: {0}")]
    FrameProcessError(frame::Error),
    #[error("Windows API error: {0}")]
    WindowsApiError(windows::core::Error),
}

impl GraphicsCaptureApiHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<Py<PyAny>>, Arc<Py<PyAny>>);
    type Error = InnerNativeWindowsCaptureError;

    #[inline]
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { on_frame_arrived_callback: ctx.flags.0, on_closed: ctx.flags.1 })
    }

    #[inline]
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let timestamp = frame.timestamp().map_err(InnerNativeWindowsCaptureError::WindowsApiError)?.Duration;
        let mut buffer = frame.buffer().map_err(InnerNativeWindowsCaptureError::FrameProcessError)?;
        let buffer = buffer.as_raw_buffer();

        Python::attach(|py| -> Result<(), Self::Error> {
            py.check_signals().map_err(InnerNativeWindowsCaptureError::PythonError)?;

            let stop_list = PyList::new(py, [false]).map_err(InnerNativeWindowsCaptureError::PythonError)?;
            self.on_frame_arrived_callback
                .call1(py, (buffer.as_ptr() as isize, buffer.len(), width, height, stop_list.clone(), timestamp))
                .map_err(InnerNativeWindowsCaptureError::PythonError)?;

            if stop_list
                .get_item(0)
                .map_err(InnerNativeWindowsCaptureError::PythonError)?
                .is_truthy()
                .map_err(InnerNativeWindowsCaptureError::PythonError)?
            {
                capture_control.stop();
            }

            Ok(())
        })?;

        Ok(())
    }

    #[inline]
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Python::attach(|py| self.on_closed.call0(py)).map_err(InnerNativeWindowsCaptureError::PythonError)?;

        Ok(())
    }
}
