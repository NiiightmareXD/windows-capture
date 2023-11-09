#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]

use std::{
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Instant,
};

use pyo3::prelude::*;
use windows::Win32::UI::WindowsAndMessaging::PostQuitMessage;
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

/// Internal Struct Used For Windows Capture
#[pyclass]
pub struct NativeWindowsCapture {
    capture_cursor: bool,
    draw_border: bool,
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
    active: Arc<AtomicBool>,
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
            active: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Start Capture
    pub fn start(&mut self) {
        let settings = WindowsCaptureSettings::new(
            Monitor::primary(),
            Some(self.capture_cursor),
            Some(self.draw_border),
            (
                self.on_frame_arrived_callback.clone(),
                self.on_closed.clone(),
                self.active.clone(),
            ),
        )
        .unwrap();

        InnerNativeWindowsCapture::start(settings).unwrap();
    }

    /// Stop Capture
    pub fn stop(&mut self) {
        println!("STOP");
        self.active.store(false, atomic::Ordering::Relaxed);
    }
}

/// Internal Capture Struct Used From NativeWindowsCapture
struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<PyObject>,
    on_closed: Arc<PyObject>,
    active: Arc<AtomicBool>,
}

impl WindowsCaptureHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<PyObject>, Arc<PyObject>, Arc<AtomicBool>);

    fn new((on_frame_arrived_callback, on_closed, active): Self::Flags) -> Self {
        Self {
            on_frame_arrived_callback,
            on_closed,
            active,
        }
    }

    fn on_frame_arrived(&mut self, mut frame: Frame) {
        if !self.active.load(atomic::Ordering::Relaxed) {
            unsafe { PostQuitMessage(0) };
            return;
        }

        let instant = Instant::now();
        let width = frame.width();
        let height = frame.height();
        let buf = frame.buffer().unwrap();
        let buf = buf.as_raw_buffer();
        let row_pitch = buf.len() / height as usize;

        Python::with_gil(|py| {
            py.check_signals().unwrap();

            self.on_frame_arrived_callback
                .call1(py, (buf.as_ptr() as isize, width, height, row_pitch))
                .unwrap();
        });

        println!("Took: {}", instant.elapsed().as_nanos() as f32 / 1000000.0);
    }

    fn on_closed(&mut self) {
        Python::with_gil(|py| self.on_closed.call0(py)).unwrap();
    }
}
