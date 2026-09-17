#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::multiple_crate_versions)] // Should update as soon as possible

use std::slice;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ::windows_capture::capture::{
    CaptureControl, CaptureControlError, Context, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use ::windows_capture::d3d11::{self, StagingTexture};
use ::windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, Error as DxgiDuplicationError};
use ::windows_capture::frame::Frame;
use ::windows_capture::graphics_capture_api::InternalCaptureControl;
use ::windows_capture::interop::{
    D3D11_MAP_READ_WRITE, D3D11_MAPPED_SUBRESOURCE, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use ::windows_capture::interop::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
};
use ::windows_capture::monitor::Monitor;
use ::windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, GraphicsCaptureItemType,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use ::windows_capture::window::Window;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyMemoryView, PyModule};

type PythonCaptureCallbacks = (Arc<Py<PyAny>>, Arc<Py<PyAny>>);

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeWindowsCapture>()?;
    m.add_class::<NativeCaptureControl>()?;
    m.add_class::<NativeDxgiDuplication>()?;
    m.add_class::<NativeMappedFrame>()?;
    m.add("NativeDxgiDuplicationFrame", m.getattr("NativeMappedFrame")?)?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GraphicsCaptureSessionOptions {
    cursor_capture: CursorCaptureSettings,
    draw_border: DrawBorderSettings,
    secondary_window: SecondaryWindowSettings,
    minimum_update_interval: MinimumUpdateIntervalSettings,
    dirty_region: DirtyRegionSettings,
}

impl GraphicsCaptureSessionOptions {
    #[inline]
    const fn settings<Flags, T>(self, item: T, flags: Flags) -> Settings<Flags, T>
    where
        T: TryInto<GraphicsCaptureItemType>,
    {
        Settings::new(
            item,
            self.cursor_capture,
            self.draw_border,
            self.secondary_window,
            self.minimum_update_interval,
            self.dirty_region,
            ColorFormat::Bgra8,
            flags,
        )
    }
}

/// Internal struct used for Windows capture.
#[pyclass]
pub struct NativeWindowsCapture {
    on_frame_arrived_callback: Arc<Py<PyAny>>,
    on_closed: Arc<Py<PyAny>>,
    session_options: GraphicsCaptureSessionOptions,
    monitor_index: Option<usize>,
    window_name: Option<String>,
    window_hwnd: Option<isize>,
}

impl NativeWindowsCapture {
    /// Builds the Rust capture settings shared by synchronous and free-threaded starts.
    #[inline]
    fn capture_settings<T>(&self, item: T) -> Settings<PythonCaptureCallbacks, T>
    where
        T: TryInto<GraphicsCaptureItemType>,
    {
        self.session_options.settings(item, (self.on_frame_arrived_callback.clone(), self.on_closed.clone()))
    }
}

#[pymethods]
impl NativeWindowsCapture {
    #[new]
    #[pyo3(signature = (on_frame_arrived_callback, on_closed, cursor_capture=None, draw_border=None, secondary_window=None, minimum_update_interval=None, dirty_region=None, monitor_index=None, window_name=None, window_hwnd=None))]
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
        window_hwnd: Option<isize>,
    ) -> PyResult<Self> {
        // Count how many capture targets are specified
        let targets_specified =
            [monitor_index.is_some(), window_name.is_some(), window_hwnd.is_some()].iter().filter(|&&x| x).count();

        if targets_specified > 1 {
            return Err(PyException::new_err(
                "You can only specify one of: monitor_index, window_name, or window_hwnd",
            ));
        }

        // Default to primary monitor if no target specified
        if targets_specified == 0 {
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
            session_options: GraphicsCaptureSessionOptions {
                cursor_capture,
                draw_border,
                secondary_window,
                minimum_update_interval,
                dirty_region: dirty_region_settings,
            },
            monitor_index,
            window_name,
            window_hwnd,
        })
    }

    /// Start capture, detaching the GIL while the native capture loop blocks.
    #[inline]
    pub fn start(&mut self, py: Python<'_>) -> PyResult<()> {
        if let Some(hwnd) = self.window_hwnd {
            // Capture by window handle (HWND)
            let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);

            let settings = self.capture_settings(window);

            match py.detach(move || InnerNativeWindowsCapture::start(settings)) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start threw an exception: {e}",
                    )));
                }
            }
        } else if let Some(ref name) = self.window_name {
            // Capture by window name (substring match)
            let window = match Window::from_contains_name(name) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to find window: {e}")));
                }
            };

            let settings = self.capture_settings(window);

            match py.detach(move || InnerNativeWindowsCapture::start(settings)) {
                Ok(()) => (),
                Err(e) => {
                    return Err(PyException::new_err(format!(
                        "InnerNativeWindowsCapture::start threw an exception: {e}",
                    )));
                }
            }
        } else {
            // Capture by monitor index
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to get monitor from index: {e}")));
                }
            };

            let settings = self.capture_settings(monitor);

            match py.detach(move || InnerNativeWindowsCapture::start(settings)) {
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

    /// Start capture on a dedicated thread, detaching the GIL during its startup handshake.
    #[inline]
    pub fn start_free_threaded(&mut self, py: Python<'_>) -> PyResult<NativeCaptureControl> {
        let capture_control = if let Some(hwnd) = self.window_hwnd {
            // Capture by window handle (HWND)
            let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);

            let settings = self.capture_settings(window);

            let capture_control = match py.detach(move || InnerNativeWindowsCapture::start_free_threaded(settings)) {
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
        } else if let Some(ref name) = self.window_name {
            // Capture by window name (substring match)
            let window = match Window::from_contains_name(name) {
                Ok(window) => window,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to find window: {e}")));
                }
            };

            let settings = self.capture_settings(window);

            let capture_control = match py.detach(move || InnerNativeWindowsCapture::start_free_threaded(settings)) {
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
            // Capture by monitor index
            let monitor = match Monitor::from_index(self.monitor_index.unwrap()) {
                Ok(monitor) => monitor,
                Err(e) => {
                    return Err(PyException::new_err(format!("Failed to get monitor from index: {e}")));
                }
            };

            let settings = self.capture_settings(monitor);

            let capture_control = match py.detach(move || InnerNativeWindowsCapture::start_free_threaded(settings)) {
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

type SharedDeviceContext = Arc<Mutex<ID3D11DeviceContext>>;

fn lock_device_context(context: &SharedDeviceContext) -> MutexGuard<'_, ID3D11DeviceContext> {
    context.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

const fn color_format_to_str(color_format: ColorFormat) -> &'static str {
    match color_format {
        ColorFormat::Bgra8 => "bgra8",
        ColorFormat::Rgba8 => "rgba8",
        ColorFormat::Rgba16F => "rgba16f",
    }
}

const fn bytes_per_pixel(color_format: ColorFormat) -> usize {
    match color_format {
        ColorFormat::Bgra8 | ColorFormat::Rgba8 => 4,
        ColorFormat::Rgba16F => 8,
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NativeMappedFrameError {
    #[error("Failed to create a staging texture: {0}")]
    StagingTexture(#[from] d3d11::Error),
    #[error("Failed to map the staging texture: {0}")]
    Map(#[source] windows_core::Error),
    #[error("The mapped staging texture returned a null data pointer")]
    NullDataPointer,
    #[error("The mapped frame size overflowed usize")]
    SizeOverflow,
}

/// Owns a mapped D3D staging texture for as long as Python retains its buffer view.
#[pyclass]
pub struct NativeMappedFrame {
    context: SharedDeviceContext,
    staging: StagingTexture,
    ptr: usize,
    len: usize,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
    row_pitch: usize,
    color_format: &'static str,
}

impl NativeMappedFrame {
    fn map_texture(
        device: &ID3D11Device,
        context: SharedDeviceContext,
        texture: &ID3D11Texture2D,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        color_format: ColorFormat,
    ) -> Result<Self, NativeMappedFrameError> {
        let mut staging = StagingTexture::new(device, width, height, format)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();

        {
            let context_guard = lock_device_context(&context);
            unsafe {
                context_guard.CopyResource(staging.texture(), texture);
                context_guard
                    .Map(staging.texture(), 0, D3D11_MAP_READ_WRITE, 0, Some(&mut mapped))
                    .ok()
                    .map_err(NativeMappedFrameError::Map)?;
            }
        }

        staging.set_mapped(true);

        let mut frame = Self {
            context,
            staging,
            ptr: mapped.pData as usize,
            len: 0,
            width,
            height,
            bytes_per_pixel: bytes_per_pixel(color_format),
            row_pitch: mapped.RowPitch as usize,
            color_format: color_format_to_str(color_format),
        };

        if frame.ptr == 0 {
            return Err(NativeMappedFrameError::NullDataPointer);
        }

        frame.len = frame
            .row_pitch
            .checked_mul(usize::try_from(height).map_err(|_| NativeMappedFrameError::SizeOverflow)?)
            .ok_or(NativeMappedFrameError::SizeOverflow)?;

        Ok(frame)
    }
}

impl Drop for NativeMappedFrame {
    fn drop(&mut self) {
        if self.staging.is_mapped() {
            let context = Arc::clone(&self.context);
            let context_guard = lock_device_context(&context);
            unsafe {
                context_guard.Unmap(self.staging.texture(), 0);
            }
            drop(context_guard);

            self.staging.set_mapped(false);
            self.ptr = 0;
            self.len = 0;
        }
    }
}

#[pymethods]
#[allow(clippy::missing_const_for_fn)]
impl NativeMappedFrame {
    #[getter]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[getter]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[getter]
    pub fn bytes_per_pixel(&self) -> usize {
        self.bytes_per_pixel
    }

    #[getter]
    pub fn color_format(&self) -> &'static str {
        self.color_format
    }

    #[getter]
    pub fn bytes_per_row(&self) -> usize {
        self.row_pitch
    }

    /// Returns the mapped pointer. It remains valid while this object or one of its buffer views exists.
    pub fn buffer_ptr(&self) -> usize {
        self.ptr
    }

    pub fn buffer_len(&self) -> usize {
        self.len
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe { slice::from_raw_parts(self.ptr as *const u8, self.len) }.to_vec()
    }

    /// Creates a zero-copy memory view whose exporter retains this native owner.
    pub fn buffer_view<'py>(slf: Bound<'py, Self>) -> PyResult<Bound<'py, PyMemoryView>> {
        let (ptr, len) = {
            let frame = slf.borrow();
            (frame.ptr, frame.len)
        };

        if ptr == 0 {
            return Err(PyException::new_err("The mapped frame has already been released"));
        }

        let py = slf.py();
        // `abi3-py39` cannot install custom buffer slots through PyO3. A ctypes array is a
        // stable-ABI buffer exporter, and its owner attribute keeps this mapped frame alive for
        // every memoryview and NumPy view derived from it.
        let ctypes = PyModule::import(py, "ctypes")?;
        let buffer_type = ctypes.getattr("ARRAY")?.call1((ctypes.getattr("c_ubyte")?, len))?;
        let buffer = buffer_type.call_method1("from_address", (ptr,))?;
        buffer.setattr("_windows_capture_owner", slf.as_any())?;

        PyMemoryView::from(&buffer)
    }
}

struct InnerNativeWindowsCapture {
    on_frame_arrived_callback: Arc<Py<PyAny>>,
    on_closed: Arc<Py<PyAny>>,
    context: Option<SharedDeviceContext>,
}

#[derive(thiserror::Error, Debug)]
pub enum InnerNativeWindowsCaptureError {
    #[error("Python callback error: {0}")]
    PythonError(pyo3::PyErr),
    #[error("Mapped frame error: {0}")]
    MappedFrameError(#[from] NativeMappedFrameError),
    #[error("Windows API error: {0}")]
    WindowsApiError(windows_core::Error),
}

impl GraphicsCaptureApiHandler for InnerNativeWindowsCapture {
    type Flags = (Arc<Py<PyAny>>, Arc<Py<PyAny>>);
    type Error = InnerNativeWindowsCaptureError;

    #[inline]
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { on_frame_arrived_callback: ctx.flags.0, on_closed: ctx.flags.1, context: None })
    }

    #[inline]
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        let timestamp = frame.timestamp().map_err(InnerNativeWindowsCaptureError::WindowsApiError)?.duration;
        let context = self.context.get_or_insert_with(|| Arc::new(Mutex::new(frame.device_context().clone()))).clone();
        let mapped_frame = NativeMappedFrame::map_texture(
            frame.device(),
            context,
            frame.as_raw_texture(),
            width,
            height,
            frame.desc().Format,
            frame.color_format(),
        )?;
        let buffer_len = mapped_frame.len;

        Python::attach(|py| -> Result<(), Self::Error> {
            py.check_signals().map_err(InnerNativeWindowsCaptureError::PythonError)?;

            let stop_list = PyList::new(py, [false]).map_err(InnerNativeWindowsCaptureError::PythonError)?;
            let mapped_frame = Py::new(py, mapped_frame).map_err(InnerNativeWindowsCaptureError::PythonError)?;
            self.on_frame_arrived_callback
                .call1(py, (mapped_frame, buffer_len, width, height, stop_list.clone(), timestamp))
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

#[pyclass(unsendable)]
pub struct NativeDxgiDuplication {
    duplication: DxgiDuplicationApi,
    context: SharedDeviceContext,
}

impl NativeDxgiDuplication {
    fn new_duplication(monitor: Monitor) -> Result<(DxgiDuplicationApi, SharedDeviceContext), DxgiDuplicationError> {
        let duplication = DxgiDuplicationApi::new(monitor)?;
        let context = Arc::new(Mutex::new(duplication.device_context().clone()));

        Ok((duplication, context))
    }

    fn color_format_from_dxgi(format: DXGI_FORMAT) -> PyResult<ColorFormat> {
        match format {
            DXGI_FORMAT_B8G8R8A8_UNORM => Ok(ColorFormat::Bgra8),
            DXGI_FORMAT_R8G8B8A8_UNORM => Ok(ColorFormat::Rgba8),
            DXGI_FORMAT_R16G16B16A16_FLOAT => Ok(ColorFormat::Rgba16F),
            other => Err(PyException::new_err(format!("Unsupported DXGI color format: {other:?}"))),
        }
    }
}

#[pymethods]
impl NativeDxgiDuplication {
    #[new]
    #[pyo3(signature = (monitor_index=None))]
    pub fn new(monitor_index: Option<usize>) -> PyResult<Self> {
        let monitor = match monitor_index {
            Some(index) => Monitor::from_index(index)
                .map_err(|e| PyException::new_err(format!("Failed to resolve monitor from index {index}: {e}",)))?,
            None => Monitor::primary()
                .map_err(|e| PyException::new_err(format!("Failed to acquire primary monitor: {e}",)))?,
        };

        let (duplication, context) = Self::new_duplication(monitor)
            .map_err(|e| PyException::new_err(format!("Failed to create DXGI duplication session: {e}")))?;

        Ok(Self { duplication, context })
    }

    #[pyo3(signature = (timeout_ms=16))]
    pub fn acquire_next_frame(&mut self, timeout_ms: u32) -> PyResult<Option<NativeMappedFrame>> {
        match self.duplication.acquire_next_frame(timeout_ms) {
            Ok(frame) => {
                let texture_desc = *frame.texture_desc();
                let width = texture_desc.Width;
                let height = texture_desc.Height;
                let color_format = Self::color_format_from_dxgi(texture_desc.Format)?;
                let frame_obj = NativeMappedFrame::map_texture(
                    frame.device(),
                    Arc::clone(&self.context),
                    frame.texture(),
                    width,
                    height,
                    texture_desc.Format,
                    color_format,
                )
                .map_err(|e| PyException::new_err(format!("Failed to map duplication frame: {e}")))?;

                Ok(Some(frame_obj))
            }
            Err(DxgiDuplicationError::Timeout) => Ok(None),
            Err(DxgiDuplicationError::AccessLost) => {
                Err(PyException::new_err("DXGI duplication access lost; call recreate() to re-establish the session"))
            }
            Err(other) => Err(PyException::new_err(format!("Failed to acquire duplication frame: {other}"))),
        }
    }
}
