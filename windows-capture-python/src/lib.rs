#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]

use std::{
    error::Error,
    os::windows::io::AsRawHandle,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    thread::JoinHandle,
};

use ::windows_capture::{
    capture::{CaptureControlError, WindowsCaptureHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, WindowsCaptureSettings},
    window::Window,
};
use pyo3::{exceptions::PyException, prelude::*, types::PyList};
use windows::Win32::{
    Foundation::{HANDLE, LPARAM, WPARAM},
    System::Threading::GetThreadId,
    UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<NativeCaptureControl>()?;
    m.add_class::<NativeWindowsCapture>()?;
    Ok(())
}

/// Internal Struct Used To Handle Free Threaded Start
#[pyclass]
pub struct NativeCaptureControl {
    capture_control: Option<NoGenericCaptureControl>,
}

impl NativeCaptureControl {
    const fn new(capture_control: NoGenericCaptureControl) -> Self {
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
            .map_or(true, |capture_control| capture_control.is_finished())
    }

    pub fn wait(&mut self, py: Python) -> PyResult<()> {
        // But Honestly WTF Is This? You Know How Much Time It Took Me To Debug This?
        // Just Why? Who Decided This BS Threading Shit?
        py.allow_threads(|| {
            if let Some(capture_control) = self.capture_control.take() {
                match capture_control.wait() {
                    Ok(_) => (),
                    Err(e) => {
                        return Err(PyException::new_err(format!(
                            "Failed To Join The Capture Thread -> {e}"
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
                    Ok(_) => (),
                    Err(e) => {
                        return Err(PyException::new_err(format!(
                            "Failed To Stop The Capture Thread -> {e}"
                        )));
                    }
                };
            }

            Ok(())
        })?;

        Ok(())
    }
}

/// Because The Default Rust Capture Control Contains Generic That Is
/// Unsupported By Python
pub struct NoGenericCaptureControl {
    thread_handle: Option<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>,
    halt_handle: Arc<AtomicBool>,
}

impl NoGenericCaptureControl {
    #[must_use]
    pub fn new(
        thread_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
        halt_handle: Arc<AtomicBool>,
    ) -> Self {
        Self {
            thread_handle: Some(thread_handle),
            halt_handle,
        }
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread_handle
            .as_ref()
            .map_or(true, |thread_handle| thread_handle.is_finished())
    }

    pub fn wait(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(thread_handle) = self.thread_handle.take() {
            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoinThread));
                }
            }
        } else {
            return Err(Box::new(CaptureControlError::ThreadHandleIsTaken));
        }

        Ok(())
    }

    /// Gracefully Stop The Capture Thread
    pub fn stop(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.halt_handle.store(true, atomic::Ordering::Relaxed);

        if let Some(thread_handle) = self.thread_handle.take() {
            let handle = thread_handle.as_raw_handle();
            let handle = HANDLE(handle as isize);
            let therad_id = unsafe { GetThreadId(handle) };

            loop {
                match unsafe {
                    PostThreadMessageW(therad_id, WM_QUIT, WPARAM::default(), LPARAM::default())
                } {
                    Ok(_) => break,
                    Err(e) => {
                        if thread_handle.is_finished() {
                            break;
                        }

                        if e.code().0 != -2147023452 {
                            Err(e)?;
                        }
                    }
                }
            }

            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoinThread));
                }
            }
        } else {
            return Err(Box::new(CaptureControlError::ThreadHandleIsTaken));
        }

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
            monitor_index = Some(0);
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

            match WindowsCaptureSettings::new(
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
            Ok(_) => (),
            Err(e) => {
                return Err(PyException::new_err(format!(
                    "Capture Session Threw An Exception -> {e}"
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

            match WindowsCaptureSettings::new(
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

            match WindowsCaptureSettings::new(
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
                return Err(PyException::new_err(format!(
                    "Failed To Start Capture Session On A Dedicated Thread -> {e}"
                )));
            }
        };

        let halt_handle = capture_control.halt_handle();
        let thread_handle = capture_control.into_thread_handle();
        let no_generic_capture_control = NoGenericCaptureControl::new(thread_handle, halt_handle);
        let capture_control = NativeCaptureControl::new(no_generic_capture_control);

        Ok(capture_control)
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
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Box<(dyn Error + Send + Sync)>> {
        let width = frame.width();
        let height = frame.height();
        let buffer = frame.buffer()?;
        let buffer = buffer.as_raw_buffer();

        Python::with_gil(|py| -> PyResult<()> {
            py.check_signals()?;

            let stop_list = PyList::new(py, [false]);
            self.on_frame_arrived_callback.call1(
                py,
                (
                    buffer.as_ptr() as isize,
                    buffer.len(),
                    width,
                    height,
                    stop_list,
                ),
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
