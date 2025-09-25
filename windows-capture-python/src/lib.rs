#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::multiple_crate_versions)] // Should update as soon as possible

use std::os::raw::{c_char, c_int};
use std::sync::Arc;
use std::time::Duration;
use std::{ptr, slice};

use ::windows_capture::capture::{
    CaptureControl, CaptureControlError, Context, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use ::windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, Error as DxgiDuplicationError};
use ::windows_capture::frame::{self, Frame};
use ::windows_capture::graphics_capture_api::InternalCaptureControl;
use ::windows_capture::monitor::Monitor;
use ::windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};
use ::windows_capture::window::Window;
use pyo3::exceptions::PyException;
use pyo3::ffi;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyMemoryView};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_SAMPLE_DESC,
};

/// Fastest Windows Screen Capture Library For Python 🔥.
#[pymodule]
fn windows_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<NativeWindowsCapture>()?;
    m.add_class::<NativeCaptureControl>()?;
    m.add_class::<NativeDxgiDuplication>()?;
    m.add_class::<NativeDxgiDuplicationFrame>()?;
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

#[pyclass(unsendable)]
pub struct NativeDxgiDuplication {
    duplication: DxgiDuplicationApi,
    monitor: Monitor,
}

impl NativeDxgiDuplication {
    fn new_duplication(monitor: Monitor) -> Result<(Monitor, DxgiDuplicationApi), DxgiDuplicationError> {
        let duplication = DxgiDuplicationApi::new(monitor)?;

        Ok((monitor, duplication))
    }

    fn recreate_duplication(&mut self) -> Result<(), DxgiDuplicationError> {
        let (_, duplication) = Self::new_duplication(self.monitor)?;
        self.duplication = duplication;
        Ok(())
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

        let (_, duplication) = Self::new_duplication(monitor)
            .map_err(|e| PyException::new_err(format!("Failed to create DXGI duplication session: {e}")))?;

        Ok(Self { duplication, monitor })
    }

    #[pyo3(signature = (timeout_ms=16))]
    pub fn acquire_next_frame(&mut self, timeout_ms: u32) -> PyResult<Option<NativeDxgiDuplicationFrame>> {
        match self.duplication.acquire_next_frame(timeout_ms) {
            Ok(frame) => {
                let texture_desc = *frame.texture_desc();
                let width = texture_desc.Width;
                let height = texture_desc.Height;
                let color_format = Self::color_format_from_dxgi(texture_desc.Format)?;
                let bytes_per_pixel = Self::bytes_per_pixel(color_format);

                let staging_desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: texture_desc.Format,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };

                let device_context = frame.device_context().clone();
                let device = frame.device().clone();

                let mut staging = None;
                unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }
                    .map_err(|e| PyException::new_err(format!("Failed to create staging texture: {e}")))?;
                let staging = staging.expect("CreateTexture2D returned Ok but no texture");

                unsafe {
                    device_context.CopyResource(&staging, frame.texture());
                }

                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                unsafe { device_context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
                    .map_err(|e| PyException::new_err(format!("Failed to map duplication frame: {e}")))?;

                let row_pitch_u32 = mapped.RowPitch;
                let row_pitch = usize::try_from(row_pitch_u32)
                    .map_err(|_| PyException::new_err("Failed to convert row pitch to usize"))?;
                let height_usize =
                    usize::try_from(height).map_err(|_| PyException::new_err("Failed to convert height to usize"))?;
                let len = row_pitch
                    .checked_mul(height_usize)
                    .ok_or_else(|| PyException::new_err("Mapped frame size overflowed usize"))?;

                let frame_obj = NativeDxgiDuplicationFrame::new(
                    device_context,
                    staging,
                    mapped.pData.cast::<u8>(),
                    len,
                    width,
                    height,
                    bytes_per_pixel,
                    row_pitch,
                    Self::color_format_to_str(color_format),
                );

                Ok(Some(frame_obj))
            }
            Err(DxgiDuplicationError::Timeout) => Ok(None),
            Err(DxgiDuplicationError::AccessLost) => {
                Err(PyException::new_err("DXGI duplication access lost; call recreate() to re-establish the session"))
            }
            Err(other) => Err(PyException::new_err(format!("Failed to acquire duplication frame: {other}"))),
        }
    }

    #[pyo3(signature = (monitor_index))]
    pub fn switch_monitor(&mut self, monitor_index: usize) -> PyResult<()> {
        let monitor = Monitor::from_index(monitor_index)
            .map_err(|e| PyException::new_err(format!("Failed to resolve monitor from index {monitor_index}: {e}")))?;

        let (_, duplication) = Self::new_duplication(monitor)
            .map_err(|e| PyException::new_err(format!("Failed to create DXGI duplication session: {e}")))?;

        self.monitor = monitor;
        self.duplication = duplication;

        Ok(())
    }

    pub fn recreate(&mut self) -> PyResult<()> {
        self.recreate_duplication()
            .map_err(|e| PyException::new_err(format!("Failed to recreate DXGI duplication session: {e}")))?;
        Ok(())
    }
}

#[pyclass(unsendable)]
pub struct NativeDxgiDuplicationFrame {
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    staging: ID3D11Texture2D,
    ptr: *mut u8,
    len: usize,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
    row_pitch: usize,
    color_format: &'static str,
    mapped: bool,
}

#[allow(clippy::missing_const_for_fn)]
impl NativeDxgiDuplicationFrame {
    #[allow(clippy::too_many_arguments)]
    fn new(
        context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        staging: ID3D11Texture2D,
        ptr: *mut u8,
        len: usize,
        width: u32,
        height: u32,
        bytes_per_pixel: usize,
        row_pitch: usize,
        color_format: &'static str,
    ) -> Self {
        Self { context, staging, ptr, len, width, height, bytes_per_pixel, row_pitch, color_format, mapped: true }
    }
}

impl Drop for NativeDxgiDuplicationFrame {
    fn drop(&mut self) {
        if self.mapped {
            unsafe {
                self.context.Unmap(&self.staging, 0);
            }
            self.mapped = false;
            self.ptr = ptr::null_mut();
            self.len = 0;
        }
    }
}

#[pymethods]
#[allow(clippy::missing_const_for_fn)]
impl NativeDxgiDuplicationFrame {
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

    pub fn buffer_ptr(&self) -> usize {
        self.ptr as usize
    }

    pub fn buffer_len(&self) -> usize {
        self.len
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        unsafe { slice::from_raw_parts(self.ptr, self.len) }.to_vec()
    }

    pub fn buffer_view<'py>(&'py self, py: Python<'py>) -> PyResult<Bound<'py, PyMemoryView>> {
        let len = isize::try_from(self.len).map_err(|_| PyException::new_err("Frame too large for memoryview"))?;
        const PYBUF_READ: c_int = 0x100;
        let view = unsafe { ffi::PyMemoryView_FromMemory(self.ptr.cast::<c_char>(), len, PYBUF_READ) };
        if view.is_null() {
            Err(PyException::new_err("Failed to create memoryview for DXGI frame"))
        } else {
            let any = unsafe { Bound::from_owned_ptr(py, view) };
            any.downcast_into().map_err(|e| e.into())
        }
    }
}
