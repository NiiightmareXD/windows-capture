use std::{
    cell::RefCell,
    error::Error,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
};

use log::{info, trace};
use parking_lot::Mutex;
use windows::{
    core::{ComInterface, IInspectable, HSTRING},
    Foundation::{Metadata::ApiInformation, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{LPARAM, WPARAM},
        Graphics::Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_TEXTURE2D_DESC,
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
        UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT},
    },
};

use crate::{
    capture::WindowsCaptureHandler,
    d3d11::{create_d3d_device, create_direct3d_device, SendDirectX},
    frame::Frame,
    settings::ColorFormat,
};

thread_local! {
    pub static RESULT: RefCell<Option<Result<(), Box<dyn Error + Send + Sync>>>> = RefCell::new(Some(Ok(())));
}

/// Used To Handle Capture Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum WindowsCaptureError {
    #[error("Graphics Capture API Is Not Supported")]
    Unsupported,
    #[error("Graphics Capture API Toggling Cursor Capture Is Not Supported")]
    CursorConfigUnsupported,
    #[error("Graphics Capture API Toggling Border Capture Is Not Supported")]
    BorderConfigUnsupported,
    #[error("Already Started")]
    AlreadyStarted,
}

/// Struct Used To Control Capture Thread
pub struct InternalCaptureControl {
    stop: Arc<AtomicBool>,
}

impl InternalCaptureControl {
    /// Create A New Capture Control Struct
    #[must_use]
    pub fn new(stop: Arc<AtomicBool>) -> Self {
        Self { stop }
    }

    /// Gracefully Stop The Capture Thread
    pub fn stop(self) {
        self.stop.store(true, atomic::Ordering::Relaxed);
    }
}

/// Struct Used For Graphics Capture Api
pub struct GraphicsCaptureApi {
    _item: GraphicsCaptureItem,
    _d3d_device: ID3D11Device,
    _direct3d_device: IDirect3DDevice,
    _d3d_device_context: ID3D11DeviceContext,
    frame_pool: Option<Arc<Direct3D11CaptureFramePool>>,
    session: Option<GraphicsCaptureSession>,
    halt: Arc<AtomicBool>,
    active: bool,
    capture_cursor: Option<bool>,
    draw_border: Option<bool>,
}

impl GraphicsCaptureApi {
    /// Create A New Graphics Capture Api Struct
    pub fn new<T: WindowsCaptureHandler + Send + 'static>(
        item: GraphicsCaptureItem,
        callback: Arc<Mutex<T>>,
        capture_cursor: Option<bool>,
        draw_border: Option<bool>,
        color_format: ColorFormat,
        thread_id: u32,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // Check Support
        if !ApiInformation::IsApiContractPresentByMajor(
            &HSTRING::from("Windows.Foundation.UniversalApiContract"),
            8,
        )? {
            return Err(Box::new(WindowsCaptureError::Unsupported));
        }

        // Create DirectX Devices
        trace!("Creating DirectX Devices");
        let (d3d_device, d3d_device_context) = create_d3d_device()?;
        let direct3d_device = create_direct3d_device(&d3d_device)?;

        let pixel_format = if color_format == ColorFormat::Rgba8 {
            DirectXPixelFormat::R8G8B8A8UIntNormalized
        } else {
            DirectXPixelFormat::B8G8R8A8UIntNormalized
        };

        // Create Frame Pool
        trace!("Creating Frame Pool");
        let frame_pool =
            Direct3D11CaptureFramePool::Create(&direct3d_device, pixel_format, 2, item.Size()?)?;
        let frame_pool = Arc::new(frame_pool);

        // Create Capture Session
        trace!("Creating Capture Session");
        let session = frame_pool.CreateCaptureSession(&item)?;

        // Preallocate Memory
        trace!("Preallocating Memory");
        let mut buffer = vec![0u8; 3840 * 2160 * 4];

        // Indicates If The Capture Is Closed
        let halt = Arc::new(AtomicBool::new(false));

        // Set Capture Session Closed Event
        item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new({
                // Init
                let callback_closed = callback.clone();
                let halt_closed = halt.clone();

                move |_, _| {
                    halt_closed.store(true, atomic::Ordering::Relaxed);

                    // Notify The Struct That The Capture Session Is Closed
                    let result = callback_closed.lock().on_closed();

                    let _ = RESULT
                        .replace(Some(result))
                        .expect("Failed To Replace RESULT");

                    // To Stop Messge Loop
                    unsafe {
                        PostThreadMessageW(
                            thread_id,
                            WM_QUIT,
                            WPARAM::default(),
                            LPARAM::default(),
                        )?;
                    };

                    Result::Ok(())
                }
            }),
        )?;

        // Set Frame Pool Frame Arrived Event
        frame_pool.FrameArrived(
            &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
                // Init
                let frame_pool_recreate = frame_pool.clone();
                let halt_frame_pool = halt.clone();
                let d3d_device_frame_pool = d3d_device.clone();
                let context = d3d_device_context.clone();

                let mut last_size = item.Size()?;
                let callback_frame_pool = callback;
                let direct3d_device_recreate = SendDirectX::new(direct3d_device.clone());

                move |frame, _| {
                    // Return Early If The Capture Is Closed
                    if halt_frame_pool.load(atomic::Ordering::Relaxed) {
                        return Ok(());
                    }

                    // Get Frame
                    let frame = frame.as_ref().unwrap().TryGetNextFrame()?;

                    // Get Frame Content Size
                    let frame_content_size = frame.ContentSize()?;

                    // Get Frame Surface
                    let frame_surface = frame.Surface()?;

                    // Convert Surface To Texture
                    let frame_surface = frame_surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
                    let frame_surface = unsafe { frame_surface.GetInterface::<ID3D11Texture2D>()? };

                    // Get Texture Settings
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    unsafe { frame_surface.GetDesc(&mut desc) }

                    // Check If The Size Has Been Changed
                    if frame_content_size.Width != last_size.Width
                        || frame_content_size.Height != last_size.Height
                    {
                        info!(
                            "Size Changed From {}x{} to {}x{} -> Recreating Device",
                            last_size.Width,
                            last_size.Height,
                            frame_content_size.Width,
                            frame_content_size.Height,
                        );
                        let direct3d_device_recreate = &direct3d_device_recreate;
                        frame_pool_recreate
                            .Recreate(
                                &direct3d_device_recreate.0,
                                pixel_format,
                                2,
                                frame_content_size,
                            )
                            .unwrap();

                        last_size = frame_content_size;

                        return Ok(());
                    }

                    // Set Width & Height
                    let texture_width = desc.Width;
                    let texture_height = desc.Height;

                    // Create A Frame
                    let mut frame = Frame::new(
                        &d3d_device_frame_pool,
                        frame_surface,
                        &context,
                        &mut buffer,
                        texture_width,
                        texture_height,
                        color_format,
                    );

                    // Init Internal Capture Control
                    let stop = Arc::new(AtomicBool::new(false));
                    let internal_capture_control = InternalCaptureControl::new(stop.clone());

                    // Send The Frame To Trigger Struct
                    let result = callback_frame_pool
                        .lock()
                        .on_frame_arrived(&mut frame, internal_capture_control);

                    if stop.load(atomic::Ordering::Relaxed) || result.is_err() {
                        let _ = RESULT
                            .replace(Some(result))
                            .expect("Failed To Replace RESULT");

                        halt_frame_pool.store(true, atomic::Ordering::Relaxed);

                        // To Stop Messge Loop
                        unsafe {
                            PostThreadMessageW(
                                thread_id,
                                WM_QUIT,
                                WPARAM::default(),
                                LPARAM::default(),
                            )?;
                        };
                    }

                    Result::Ok(())
                }
            }),
        )?;

        Ok(Self {
            _item: item,
            _d3d_device: d3d_device,
            _direct3d_device: direct3d_device,
            _d3d_device_context: d3d_device_context,
            frame_pool: Some(frame_pool),
            session: Some(session),
            halt,
            active: false,
            capture_cursor,
            draw_border,
        })
    }

    /// Start Capture
    pub fn start_capture(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Check If The Capture Is Already Installed
        if self.active {
            return Err(Box::new(WindowsCaptureError::AlreadyStarted));
        }

        // Config
        if self.capture_cursor.is_some() {
            if ApiInformation::IsPropertyPresent(
                &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
                &HSTRING::from("IsCursorCaptureEnabled"),
            )? {
                self.session
                    .as_ref()
                    .unwrap()
                    .SetIsCursorCaptureEnabled(self.capture_cursor.unwrap())?;
            } else {
                return Err(Box::new(WindowsCaptureError::CursorConfigUnsupported));
            }
        }

        if self.draw_border.is_some() {
            if ApiInformation::IsPropertyPresent(
                &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
                &HSTRING::from("IsBorderRequired"),
            )? {
                self.session
                    .as_ref()
                    .unwrap()
                    .SetIsBorderRequired(self.draw_border.unwrap())?;
            } else {
                return Err(Box::new(WindowsCaptureError::BorderConfigUnsupported));
            }
        }

        // Start Capture
        self.session.as_ref().unwrap().StartCapture()?;

        self.active = true;

        Ok(())
    }

    /// Stop Capture
    pub fn stop_capture(mut self) {
        if let Some(frame_pool) = self.frame_pool.take() {
            frame_pool.Close().expect("Failed to Close Frame Pool");
        }

        if let Some(session) = self.session.take() {
            session.Close().expect("Failed to Close Capture Session");
        }
    }

    /// Get Halt Handle
    #[must_use]
    pub fn halt_handle(&self) -> Arc<AtomicBool> {
        self.halt.clone()
    }

    /// Check If Windows Graphics Capture Api Is Supported
    pub fn is_supported() -> Result<bool, Box<dyn Error + Send + Sync>> {
        Ok(ApiInformation::IsApiContractPresentByMajor(
            &HSTRING::from("Windows.Foundation.UniversalApiContract"),
            8,
        )?)
    }

    /// Check If You Can Toggle The Cursor On Or Off
    pub fn is_cursor_toggle_supported() -> Result<bool, Box<dyn Error + Send + Sync>> {
        Ok(ApiInformation::IsPropertyPresent(
            &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
            &HSTRING::from("IsCursorCaptureEnabled"),
        )?)
    }

    /// Check If You Can Toggle The Border On Or Off
    pub fn is_border_toggle_supported() -> Result<bool, Box<dyn Error + Send + Sync>> {
        Ok(ApiInformation::IsPropertyPresent(
            &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
            &HSTRING::from("IsBorderRequired"),
        )?)
    }
}

// Close Capture Session
impl Drop for GraphicsCaptureApi {
    fn drop(&mut self) {
        if let Some(frame_pool) = self.frame_pool.take() {
            frame_pool.Close().expect("Failed to Close Frame Pool");
        }

        if let Some(session) = self.session.take() {
            session.Close().expect("Failed to Close Capture Session");
        }
    }
}
