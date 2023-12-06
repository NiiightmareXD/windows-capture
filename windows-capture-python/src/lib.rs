#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::redundant_pub_crate)]

use std::sync::Arc;

use ::windows_capture::{
    capture::{CaptureControl, CaptureControlError, WindowsCaptureError, WindowsCaptureHandler},
    frame::{self, Frame},
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, Settings},
    window::Window,
};
use pyo3::{exceptions::PyException, prelude::*, types::PyList};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(_py: Python, m: &PyModule) -> PyResult<()> {
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
    pub fn is_finished(&self) -> bool {
        self.capture_control
            .as_ref()
            .map_or(true, CaptureControl::is_finished)
    }

    pub fn wait(&mut self, py: Python) -> PyResult<()> {
        // But Honestly WTF Is This? You Know How Much Time It Took Me To Debug This?
        // Just Why? Who Decided This BS Threading Shit?
        py.allow_threads(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.wait() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::WindowsCaptureError(
                            WindowsCaptureError::FrameHandlerError(
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

    pub fn stop(&mut self, py: Python) -> PyResult<()> {
        // But Honestly WTF Is This? You Know How Much Time It Took Me To Debug This?
        // Just Why? Who Decided This BS Threading Shit?
        py.allow_threads(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.stop() {
                    Ok(()) => (),
                    Err(e) => {
                        if let CaptureControlError::WindowsCaptureError(
                            WindowsCaptureError::FrameHandlerError(
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
    capture_cursor: Option<bool>,
    draw_border: Option<bool>,
    monitor_index: Option<usize>,
    window_name: Option<String>,
}

#[pymethods]
impl NativeWindowsCapture {
    #[new]
    pub fn new(
        on_frame_arrived_callback: PyObject,
        on_closed: PyObject,
        capture_cursor: Option<bool>,
        draw_border: Option<bool>,
        mut monitor_index: Option<usize>,
        window_name: Option<String>,
    ) -> PyResult<Self> {
        if window_name.is_some() && monitor_index.is_some() {
            return Err(PyException::new_err(
                "You Can't Specify Both The Monitor Index And The Window Name",
            ));
        }

        if window_name.is_none() && monitor_index.is_none() {
            monitor_index = Some(1);
        }

        Ok(Self {
            on_frame_arrived_callback: Arc::new(on_frame_arrived_callback),
            on_closed: Arc::new(on_closed),
            capture_cursor,
            draw_border,
            monitor_index,
            window_name,
        })
    }

    /// Start Capture
    pub fn start(&mut self) -> PyResult<()> {
        let settings = if self.window_name.is_some() {
            let window = match Window::from_contains_name(self.window_name.as_ref().unwrap()) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Find Window -> {e}"
                    )));
                }
            };

            match Settings::new(
                window,
                self.capture_cursor,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            ) {
                Ok(settings) => settings,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Create Windows Capture Settings -> {e}"
                    )));
                }
            }
        } else {
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Get Monitor From Index -> {e}"
                    )));
                }
            };

            match Settings::new(
                monitor,
                self.capture_cursor,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            ) {
                Ok(settings) => settings,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Create Windows Capture Settings -> {e}"
                    )));
                }
            }
        };

        match InnerNativeWindowsCapture::start(settings) {
            Ok(()) => (),
            Err(e) => {
                if let WindowsCaptureError::FrameHandlerError(
                    InnerNativeWindowsCaptureError::PythonError(ref e),
                ) = e
                {
                    return Err(PyException::new_err(format!(
                        "Capture Session Threw An Exception -> {e}",
                    )));
                }

                return Err(PyException::new_err(format!(
                    "Capture Session Threw An Exception -> {e}",
                )));
            }
        }

        Ok(())
    }

    /// Start Capture On A Dedicated Thread
    pub fn start_free_threaded(&mut self) -> PyResult<NativeCaptureControl> {
        let settings = if self.window_name.is_some() {
            let window = match Window::from_contains_name(self.window_name.as_ref().unwrap()) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Find Window -> {e}"
                    )));
                }
            };

            match Settings::new(
                window,
                self.capture_cursor,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            ) {
                Ok(settings) => settings,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Create Windows Capture Settings -> {e}"
                    )));
                }
            }
        } else {
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Get Monitor From Index -> {e}"
                    )));
                }
            };

            match Settings::new(
                monitor,
                self.capture_cursor,
                self.draw_border,
                ColorFormat::Bgra8,
                (
                    self.on_frame_arrived_callback.clone(),
                    self.on_closed.clone(),
                ),
            ) {
                Ok(settings) => settings,
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "Failed To Create Windows Capture Settings -> {e}"
                    )));
                }
            }
        };

        let capture_control = match InnerNativeWindowsCapture::start_free_threaded(settings) {
            Ok(capture_control) => capture_control,
            Err(e) => {
                if let WindowsCaptureError::FrameHandlerError(
                    InnerNativeWindowsCaptureError::PythonError(ref e),
                ) = e
                {
                    return Err(PyException::new_err(format!(
                        "Capture Session Threw An Exception -> {e}",
                    )));
                }

                return Err(PyException::new_err(format!(
                    "Capture Session Threw An Exception -> {e}",
                )));
            }
        };

        let capture_control = NativeCaptureControl::new(capture_control);

        Ok(capture_control)
    }
}

struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
}

#[derive(thiserror::Error, Debug)]
pub enum InnerNativeWindowsCaptureError {
    #[error("Python Callback Error")]
    PythonError(pyo3::PyErr),
    #[error("Frame Process Error")]
    FrameProcessError(frame::Error),
}

impl WindowsCaptureHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<PyObject>, Arc<PyObject>);
    type Error = InnerNativeWindowsCaptureError;

    fn new((on_frame_arrived_callback, on_closed): Self::Flags) -> Result<Self, Self::Error> {
        Ok(Self {
            on_frame_arrived_callback,
            on_closed,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let mut buffer = frame
            .buffer()
            .map_err(InnerNativeWindowsCaptureError::FrameProcessError)?;
        let buffer = buffer.as_raw_buffer();

        Python::with_gil(|py| -> Result<(), Self::Error> {
            py.check_signals()
                .map_err(InnerNativeWindowsCaptureError::PythonError)?;

            let stop_list = PyList::new(py, [false]);
            self.on_frame_arrived_callback
                .call1(
                    py,
                    (
                        buffer.as_ptr() as isize,
                        buffer.len(),
                        width,
                        height,
                        stop_list,
                    ),
                )
                .map_err(InnerNativeWindowsCaptureError::PythonError)?;

            if stop_list[0]
                .is_true()
                .map_err(InnerNativeWindowsCaptureError::PythonError)?
            {
                capture_control.stop();
            }

            Ok(())
        })?;

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Python::with_gil(|py| self.on_closed.call0(py))
            .map_err(InnerNativeWindowsCaptureError::PythonError)?;

        Ok(())
    }
}
