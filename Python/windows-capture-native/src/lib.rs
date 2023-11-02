#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]

use std::{sync::Arc, time::Instant};

use pyo3::{
    prelude::*,
    types::{PyBytes, PyTuple},
};
use windows_capture::{
    capture::WindowsCaptureHandler, frame::Frame, monitor::Monitor,
    settings::WindowsCaptureSettings,
};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture_native(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<NativeWindowsCapture>()?;
    Ok(())
}

#[pyclass]
pub struct NativeWindowsCapture {
    capture_cursor: bool,
    draw_border: bool,
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
}

#[pymethods]
impl NativeWindowsCapture {
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

    pub fn start(&mut self) {
        let settings = WindowsCaptureSettings::new(
            Monitor::primary(),
            Some(self.capture_cursor),
            Some(self.draw_border),
            (
                self.on_frame_arrived_callback.clone(),
                self.on_closed.clone(),
            ),
        )
        .unwrap();

        InnerNativeWindowsCapture::start(settings).unwrap();
    }
}

pub struct InnerNativeWindowsCapture {
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

    fn on_frame_arrived(&mut self, mut frame: Frame) {
        let instant = Instant::now();
        let buf = frame.buffer().unwrap();

        let buf_bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), std::mem::size_of_val(buf))
        };

        Python::with_gil(|py| {
            let buf_pybytes = PyBytes::new(py, buf_bytes);

            let args = PyTuple::new(py, [buf_pybytes]);

            self.on_frame_arrived_callback.call1(py, args)
        })
        .unwrap();

        println!("Took: {}", instant.elapsed().as_nanos() as f32 / 1000000.0);
    }

    fn on_closed(&mut self) {
        Python::with_gil(|py| self.on_closed.call0(py)).unwrap();
    }
}
