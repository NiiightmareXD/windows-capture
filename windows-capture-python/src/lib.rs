#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::multiple_crate_versions)] // Should update as soon as possible

use std::sync::Arc;

use ::windows_capture::{
    capture::{
        CaptureControl, CaptureControlError, Context, GraphicsCaptureApiError,
        GraphicsCaptureApiHandler,
    },
    frame::{self, Frame},
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings},
    window::Window,
};
use pyo3::{exceptions::PyException, prelude::*, types::PyList};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeWindowsCapture>()?;
    m.add_class::<NativeCaptureControl>()?;
    Ok(())
}

/// Internal Struct Used To Handle Free Threaded Start
#[pyclass]
pub struct NativeCaptureControl {
    capture_control:
        Option<CaptureControl<InnerNativeWindowsCapture, InnerNativeWindowsCaptureError>>,
}

impl NativeCaptureControl {
    #[must_use]
    #[inline]
    const fn new(
        capture_control: CaptureControl<InnerNativeWindowsCapture, InnerNativeWindowsCaptureError>,
    ) -> Self {
        Self {
            capture_control: Some(capture_control),
        }
    }
}

#[pymethods]
impl NativeCaptureControl {
    #[must_use]
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.capture_control
            .as_ref()
            .map_or(true, CaptureControl::is_finished)
    }

    #[inline]
    pub fn wait(&mut self, py: Python) -> PyResult<()> {
        // But Honestly WTF Is This? You Know How Much Time It Took Me To Debug This?
        // Just Why? Who Decided This BS Threading Shit?
        py.allow_threads(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.wait() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::GraphicsCaptureApiError(
                            GraphicsCaptureApiError::FrameHandlerError(
                                InnerNativeWindowsCaptureError::PythonError(ref e),
                            ),
                        ) = e
                        {
                            return Err(PyException::new_err(format!(
                                "Failed To Join The Capture Thread -> {e}",
                            )));
                        }

                        return Err(PyException::new_err(format!(
                            "Failed To Join The Capture Thread -> {e}",
                        )));
                    }
                };
            }

            Ok(())
        })?;

        Ok(())
    }

    #[inline]
    pub fn stop(&mut self, py: Python) -> PyResult<()> {
        // But Honestly WTF Is This? You Know How Much Time It Took Me To Debug This?
        // Just Why? Who TF Decided This BS Threading Shit?
        py.allow_threads(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.stop() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::GraphicsCaptureApiError(
                            GraphicsCaptureApiError::FrameHandlerError(
                                InnerNativeWindowsCaptureError::PythonError(ref e),
                            ),
                        ) = e
                        {
                            return Err(PyException::new_err(format!(
                                "Failed To Stop The Capture Thread -> {e}",
                            )));
                        }

                        return Err(PyException::new_err(format!(
                            "Failed To Stop The Capture Thread -> {e}",
                        )));
                    }
                };
            }

            Ok(())
        })?;

        Ok(())
    }
}

/// Internal Struct Used For Windows Capture
#[pyclass]
pub struct NativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
    cursor_capture: CursorCaptureSettings,
    draw_border: DrawBorderSettings,
    monitor_index: Option<usize>,
    window_name: Option<String>,
    window_handle: Option<isize>,
}

#[pymethods]
impl NativeWindowsCapture {
    #[new]
    #[pyo3(signature = (on_frame_arrived_callback, on_closed, cursor_capture=None, draw_border=None, monitor_index=None, window_name=None, window_handle=None))]
    #[inline]
    pub fn new(
        on_frame_arrived_callback: PyObject,
        on_closed: PyObject,
        cursor_capture: Option<bool>,
        draw_border: Option<bool>,
        mut monitor_index: Option<usize>,
        window_name: Option<String>,
        window_handle: Option<isize>,
    ) -> PyResult<Self> {
        // Ensure only one of window_name, monitor_index, or window_handle is provided
        if [window_name.is_some(), monitor_index.is_some(), window_handle.is_some()]
            .iter()
            .filter(|&&v| v)
            .count()
            > 1
        {
            return Err(PyException::new_err(
                "You can only specify one of window_name, monitor_index, or window_handle.",
            ));
        }

        if window_name.is_none() && monitor_index.is_none() && window_handle.is_none() {
            monitor_index = Some(1); // Default to monitor index 1 if nothing is provided
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

        Ok(Self {
            on_frame_arrived_callback: Arc::new(on_frame_arrived_callback),
            on_closed: Arc::new(on_closed),
            cursor_capture,
            draw_border,
            monitor_index,
            window_name,
            window_handle,
        })
    }

    /// Start Capture
    #[inline]
    pub fn start(&mut self) -> PyResult<()> {
        if let Some(window_handle) = self.window_handle {
            // Use the window handle to create a Window instance
            let window = Window::from_raw_hwnd(window_handle as *mut std::ffi::c_void);

            // Create settings for the capture session
            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            // Start the capture session
            match InnerNativeWindowsCapture::start(settings) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start failed -> {e}",
                    )));
                }
            }
        } else if let Some(window_name) = &self.window_name {
            // Capture using window_name
            let window = match Window::from_contains_name(window_name) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to find window by name -> {e}"
                    )));
                }
            };

            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            match InnerNativeWindowsCapture::start(settings) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start failed -> {e}",
                    )));
                }
            }
        } else if let Some(monitor_index) = self.monitor_index {
            // Capture using monitor_index
            let monitor = match Monitor::from_index(monitor_index) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to get monitor by index -> {e}"
                    )));
                }
            };

            let settings = Settings::new(
                monitor,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            match InnerNativeWindowsCapture::start(settings) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start failed -> {e}",
                    )));
                }
            }
        }

        Ok(())
    }

    /// Start Capture On A Dedicated Thread
    #[inline]
    pub fn start_free_threaded(&mut self) -> PyResult<NativeCaptureControl> {
        let capture_control = if let Some(window_handle) = self.window_handle {
            // Create a Window instance from the raw window handle
            let window = Window::from_raw_hwnd(window_handle as *mut std::ffi::c_void);

            // Create settings for capture session
            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            // Start capture session on a dedicated thread
            match InnerNativeWindowsCapture::start_free_threaded(settings) {
                Ok(capture_control) => capture_control,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to start capture with window_handle -> {e}"
                    )));
                }
            }
        } else if let Some(window_name) = &self.window_name {
            // Create a Window instance from the window name
            let window = match Window::from_contains_name(window_name) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to find window by name -> {e}"
                    )));
                }
            };

            // Create settings for capture session
            let settings = Settings::new(
                window,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            // Start capture session on a dedicated thread
            match InnerNativeWindowsCapture::start_free_threaded(settings) {
                Ok(capture_control) => capture_control,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to start capture with window_name -> {e}"
                    )));
                }
            }
        } else if let Some(monitor_index) = self.monitor_index {
            // Create a Monitor instance from the monitor index
            let monitor = match Monitor::from_index(monitor_index) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to get monitor by index -> {e}"
                    )));
                }
            };

            // Create settings for capture session
            let settings = Settings::new(
                monitor,
                self.cursor_capture,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            );

            // Start capture session on a dedicated thread
            match InnerNativeWindowsCapture::start_free_threaded(settings) {
                Ok(capture_control) => capture_control,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed to start capture with monitor_index -> {e}"
                    )));
                }
            }
        } else {
            return Err(PyException::new_err(
                "No valid capture source specified. Please specify window_handle, window_name, or monitor_index.",
            ));
        };

        // Wrap the capture control in a Python-compatible wrapper and return it
        Ok(NativeCaptureControl::new(capture_control))
    }
}

struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
}

#[derive(thiserror::Error, Debug)]
pub enum InnerNativeWindowsCaptureError {
    #[error("Python Callback Error: {0}")]
    PythonError(pyo3::PyErr),
    #[error("Frame Process Error: {0}")]
    FrameProcessError(frame::Error),
}

impl GraphicsCaptureApiHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<PyObject>, Arc<PyObject>);
    type Error = InnerNativeWindowsCaptureError;

    #[inline]
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            on_frame_arrived_callback: ctx.flags.0,
            on_closed: ctx.flags.1,
        })
    }

    #[inline]
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let timespan = frame.timespan().Duration;
        let mut buffer = frame
            .buffer()
            .map_err(InnerNativeWindowsCaptureError::FrameProcessError)?;
        let buffer = buffer.as_raw_buffer();

        Python::with_gil(|py| -> Result<(), Self::Error> {
            py.check_signals()
                .map_err(InnerNativeWindowsCaptureError::PythonError)?;

            let stop_list = PyList::new_bound(py, [false]);
            self.on_frame_arrived_callback
                .call1(
                    py,
                    (
                        buffer.as_ptr() as isize,
                        buffer.len(),
                        width,
                        height,
                        stop_list.clone(),
                        timespan,
                    ),
                )
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
        Python::with_gil(|py| self.on_closed.call0(py))
            .map_err(InnerNativeWindowsCaptureError::PythonError)?;

        Ok(())
    }
}
