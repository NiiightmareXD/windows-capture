#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]

use std::sync::Arc;

use ::windows_capture::{
    capture::WindowsCaptureHandler,
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, WindowsCaptureSettings},
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
    capture_cursor: bool,
    draw_border: bool,
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
}

#[pymethods]
impl NativeWindowsCapture {
    /// Create A New Windows Capture Struct
    #[new]
    #[must_use]
    pub fn new(
        capture_cursor: bool,
        draw_border: bool,
        on_frame_arrived_callback: PyObject,
        on_closed: PyObject,
    ) -> Self {
        Self {
            capture_cursor,
            draw_border,
            on_frame_arrived_callback: Arc::new(on_frame_arrived_callback),
            on_closed: Arc::new(on_closed),
        }
    }

    /// Start Capture
    pub fn start(&mut self) -> PyResult<()> {
        let settings = match WindowsCaptureSettings::new(
            Monitor::primary(),
            Some(self.capture_cursor),
            Some(self.draw_border),
            ColorFormat::Bgra8,
            (
                self.on_frame_arrived_callback.clone(),
                self.on_closed.clone(),
            ),
        ) {
            Ok(settings) => settings,
            Err(e) => Err(PyException::new_err(format!(
                "Failed To Create Windows Capture Settings -> {e}"
            )))?,
        };

        match InnerNativeWindowsCapture::start(settings) {
            Ok(_) => (),
            Err(e) => Err(PyException::new_err(format!(
                "Capture Session Threw An Exception -> {e}"
            )))?,
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

    fn new((on_frame_arrived_callback, on_closed): Self::Flags) -> Self {
        Self {
            on_frame_arrived_callback,
            on_closed,
        }
    }

    fn on_frame_arrived(&mut self, mut frame: Frame, capture_control: InternalCaptureControl) {
        let width = frame.width();
        let height = frame.height();
        let buf = match frame.buffer() {
            Ok(buf) => buf,
            Err(e) => {
                error!(
                    "Failed To Get Frame Buffer -> {e} -> Gracefully Stopping The Capture Thread"
                );
                capture_control.stop();
                return;
            }
        };

        let buf = buf.as_raw_buffer();

        Python::with_gil(|py| {
            match py.check_signals() {
                Ok(_) => (),
                Err(_) => {
                    info!("KeyboardInterrupt Detected -> Gracefully Stopping The Capture Thread");
                    capture_control.stop();
                    return;
                }
            }

            let stop_list = PyList::new(py, [false]);
            match self.on_frame_arrived_callback.call1(
                py,
                (buf.as_ptr() as isize, buf.len(), width, height, stop_list),
            ) {
                Ok(_) => (),
                Err(e) => {
                    error!(
                        "on_frame_arrived Threw An Exception -> {e} -> Gracefully Stopping The \
                         Capture Thread"
                    );
                    capture_control.stop();
                    return;
                }
            };

            if stop_list[0].is_true().unwrap_or(false) {
                capture_control.stop();
            }
        });
    }

    fn on_closed(&mut self) {
        Python::with_gil(|py| match self.on_closed.call0(py) {
            Ok(_) => (),
            Err(e) => error!("on_closed Threw An Exception -> {e}"),
        });
    }
}
