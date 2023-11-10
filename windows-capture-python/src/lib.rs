#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]

use std::{error::Error, sync::Arc};

use ::windows_capture::{
    capture::WindowsCaptureHandler,
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, WindowsCaptureSettings},
    window::Window,
};
use log::{error, info};
use pyo3::{exceptions::PyException, prelude::*, types::PyList};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(_py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();

    m.add_class::<NativeWindowsCapture>()?;
    Ok(())
}

/// Internal Struct Used For Windows Capture
#[pyclass]
pub struct NativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
    capture_cursor: bool,
    draw_border: bool,
    monitor_index: Option<usize>,
    window_name: Option<String>,
}

#[pymethods]
impl NativeWindowsCapture {
    /// Create A New Windows Capture Struct
    #[new]
    pub fn new(
        on_frame_arrived_callback: PyObject,
        on_closed: PyObject,
        capture_cursor: bool,
        draw_border: bool,
        monitor_index: Option<usize>,
        window_name: Option<String>,
    ) -> PyResult<Self> {
        if window_name.is_some() && monitor_index.is_some() {
            return Err(PyException::new_err(
                "You Can't Specify Both The Monitor Index And The Window Name",
            ));
        }

        if window_name.is_none() && monitor_index.is_none() {
            return Err(PyException::new_err(
                "You Should Specify Either The Monitor Index Or The Window Name",
            ));
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

            match WindowsCaptureSettings::new(
                window,
                Some(self.capture_cursor),
                Some(self.draw_border),
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

            match WindowsCaptureSettings::new(
                monitor,
                Some(self.capture_cursor),
                Some(self.draw_border),
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
            Ok(_) => (),
            Err(e) => {
                return Err(PyException::new_err(format!(
                    "Capture Session Threw An Exception -> {e}"
                )));
            }
        }

        Ok(())
    }
}

struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
}

impl WindowsCaptureHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<PyObject>, Arc<PyObject>);

    fn new(
        (on_frame_arrived_callback, on_closed): Self::Flags,
    ) -> Result<Self, Box<(dyn Error + Send + Sync)>> {
        Ok(Self {
            on_frame_arrived_callback,
            on_closed,
        })
    }

    fn on_frame_arrived(
        &mut self,
        mut frame: Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Box<(dyn Error + Send + Sync)>> {
        let width = frame.width();
        let height = frame.height();
        let buf = match frame.buffer() {
            Ok(buf) => buf,
            Err(e) => {
                error!(
                    "Failed To Get Frame Buffer -> {e} -> Gracefully Stopping The Capture Thread"
                );
                capture_control.stop();
                return Ok(());
            }
        };

        let buf = buf.as_raw_buffer();

        Python::with_gil(|py| -> PyResult<()> {
            match py.check_signals() {
                Ok(_) => (),
                Err(_) => {
                    info!("KeyboardInterrupt Detected -> Gracefully Stopping The Capture Thread");
                    capture_control.stop();
                    return Ok(());
                }
            }

            let stop_list = PyList::new(py, [false]);
            self.on_frame_arrived_callback.call1(
                py,
                (buf.as_ptr() as isize, buf.len(), width, height, stop_list),
            )?;

            if stop_list[0].is_true()? {
                capture_control.stop();
            }

            Ok(())
        })?;

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Box<(dyn Error + Send + Sync)>> {
        Python::with_gil(|py| self.on_closed.call0(py))?;

        Ok(())
    }
}
