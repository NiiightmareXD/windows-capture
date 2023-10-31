use std::{
    alloc::{self, Layout},
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
};

use log::{info, trace};
use parking_lot::Mutex;
use thiserror::Error;
use windows::{
    core::{ComInterface, IInspectable, HSTRING},
    Foundation::{Metadata::ApiInformation, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Graphics::{
            Direct3D11::{
                ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
        UI::WindowsAndMessaging::PostQuitMessage,
    },
};

use crate::{
    buffer::{Buffer, SendBuffer},
    capture::WindowsCaptureHandler,
    d3d11::{create_d3d_device, create_direct3d_device, SendDirectX},
    frame::Frame,
};

/// Used To Handle Internal Capture Errors
#[derive(Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum WindowsCaptureError {
    #[error("Graphics Capture API Is Not Supported")]
    Unsupported,
    #[error("Graphics Capture API Changing Cursor Status Is Not Supported")]
    CursorUnsupported,
    #[error("Graphics Capture API Changing Border Status Is Not Supported")]
    BorderUnsupported,
    #[error("Already Started")]
    AlreadyStarted,
    #[error("Capture Session Is Closed")]
    CaptureClosed,
}

/// Struct To Use For Graphics Capture Api
pub struct GraphicsCaptureApi {
    _item: GraphicsCaptureItem,
    _d3d_device: ID3D11Device,
    _direct3d_device: IDirect3DDevice,
    _d3d_device_context: ID3D11DeviceContext,
    buffer: Buffer,
    frame_pool: Option<Arc<Direct3D11CaptureFramePool>>,
    session: Option<GraphicsCaptureSession>,
    active: bool,
    closed: bool,
}

impl GraphicsCaptureApi {
    /// Create A New Graphics Capture Api Struct
    pub fn new<T: WindowsCaptureHandler + std::marker::Send + 'static>(
        item: GraphicsCaptureItem,
        callback: T,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Check Support
        if !ApiInformation::IsApiContractPresentByMajor(
            &HSTRING::from("Windows.Foundation.UniversalApiContract"),
            8,
        )? {
            return Err(Box::new(WindowsCaptureError::Unsupported));
        }

        // Allocate 8MB Of Memory
        trace!("Allocating 8MB Of Memory");
        let layout = Layout::new::<[u8; 8 * 1024 * 1024]>();
        let ptr = unsafe { alloc::alloc(layout) };
        if ptr.is_null() {
            alloc::handle_alloc_error(layout);
        }

        let buffer = Buffer::new(ptr, layout);

        // Create DirectX Devices
        trace!("Creating DirectX Devices");
        let (d3d_device, d3d_device_context) = create_d3d_device()?;
        let direct3d_device = create_direct3d_device(&d3d_device)?;

        // Create Frame Pool
        trace!("Creating Frame Pool");
        let frame_pool = Direct3D11CaptureFramePool::Create(
            &direct3d_device,
            DirectXPixelFormat::R8G8B8A8UIntNormalized,
            1,
            item.Size()?,
        )?;
        let frame_pool = Arc::new(frame_pool);

        // Create Capture Session
        trace!("Creating Capture Session");
        let session = frame_pool.CreateCaptureSession(&item)?;

        // Trigger Struct
        let callback = Arc::new(Mutex::new(callback));

        // Indicates If The Capture Is Closed
        let closed = Arc::new(AtomicBool::new(false));

        // Set Capture Session Closed Event
        item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new({
                // Init
                let callback_closed = callback.clone();
                let closed_item = closed.clone();

                move |_, _| {
                    unsafe { PostQuitMessage(0) };

                    callback_closed.lock().on_closed();

                    closed_item.store(true, atomic::Ordering::Relaxed);

                    Result::Ok(())
                }
            }),
        )?;

        // Set Frame Pool Frame Arrived Event
        frame_pool.FrameArrived(
            &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
                // Init
                let frame_pool_recreate = frame_pool.clone();
                let closed_frame_pool = closed;
                let d3d_device_frame_pool = d3d_device.clone();
                let context = d3d_device_context.clone();

                let mut last_size = item.Size()?;
                let callback_frame_arrived = callback;
                let direct3d_device_recreate = SendDirectX::new(direct3d_device.clone());

                let buffer = SendBuffer::new(buffer);
                move |frame, _| {
                    // Return Early If The Capture Is Closed
                    if closed_frame_pool.load(atomic::Ordering::Relaxed) {
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

                    // Check Frame Format
                    if desc.Format == DXGI_FORMAT_R8G8B8A8_UNORM {
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
                                    DirectXPixelFormat::R8G8B8A8UIntNormalized,
                                    1,
                                    frame_content_size,
                                )
                                .unwrap();

                            last_size = frame_content_size;

                            return Ok(());
                        }

                        // Set Width & Height
                        let texture_width = desc.Width;
                        let texture_height = desc.Height;

                        // Texture Settings
                        let texture_desc = D3D11_TEXTURE2D_DESC {
                            Width: texture_width,
                            Height: texture_height,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_STAGING,
                            BindFlags: 0,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                            MiscFlags: 0,
                        };

                        // Create A Texture That CPU Can Read
                        let mut texture = None;
                        unsafe {
                            d3d_device_frame_pool.CreateTexture2D(
                                &texture_desc,
                                None,
                                Some(&mut texture),
                            )?;
                        };
                        let texture = texture.unwrap();

                        let buffer = &buffer;
                        let frame = Frame::new(
                            buffer.0,
                            texture,
                            frame_surface,
                            context.clone(),
                            texture_width,
                            texture_height,
                        );

                        // Send The Frame To Trigger Struct
                        callback_frame_arrived.lock().on_frame_arrived(frame);
                    } else {
                        callback_frame_arrived.lock().on_closed();

                        unsafe { PostQuitMessage(0) };
                        closed_frame_pool.store(true, atomic::Ordering::Relaxed);
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
            buffer,
            frame_pool: Some(frame_pool),
            session: Some(session),
            active: false,
            closed: false,
        })
    }

    /// Start Capture
    pub fn start_capture(
        &mut self,
        capture_cursor: Option<bool>,
        draw_border: Option<bool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Check If The Capture Is Already Installed
        if self.active {
            return Err(Box::new(WindowsCaptureError::AlreadyStarted));
        }

        if self.closed {
            return Err(Box::new(WindowsCaptureError::CaptureClosed));
        }

        // Config
        if capture_cursor.is_some() {
            if ApiInformation::IsPropertyPresent(
                &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
                &HSTRING::from("IsCursorCaptureEnabled"),
            )? {
                self.session
                    .as_ref()
                    .unwrap()
                    .SetIsCursorCaptureEnabled(capture_cursor.unwrap())?;
            } else {
                return Err(Box::new(WindowsCaptureError::CursorUnsupported));
            }
        }

        if draw_border.is_some() {
            if ApiInformation::IsPropertyPresent(
                &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
                &HSTRING::from("IsBorderRequired"),
            )? {
                self.session
                    .as_ref()
                    .unwrap()
                    .SetIsBorderRequired(draw_border.unwrap())?;
            } else {
                return Err(Box::new(WindowsCaptureError::BorderUnsupported));
            }
        }

        // Start Capture
        self.session.as_ref().unwrap().StartCapture()?;

        self.active = true;

        Ok(())
    }

    /// Stop Capture
    pub fn stop_capture(&mut self) {
        self.closed = true;

        if let Some(frame_pool) = self.frame_pool.take() {
            frame_pool.Close().expect("Failed to Close Frame Pool");
        }

        if let Some(session) = self.session.take() {
            session.Close().expect("Failed to Close Capture Session");
        }
    }

    /// Check If Windows Graphics Capture Api Is Supported
    pub fn is_supported() -> Result<bool, Box<dyn std::error::Error>> {
        Ok(ApiInformation::IsApiContractPresentByMajor(
            &HSTRING::from("Windows.Foundation.UniversalApiContract"),
            8,
        )?)
    }

    /// Check If You Can Toggle The Cursor On Or Off
    pub fn is_cursor_toggle_supported() -> Result<bool, Box<dyn std::error::Error>> {
        Ok(ApiInformation::IsPropertyPresent(
            &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
            &HSTRING::from("IsCursorCaptureEnabled"),
        )?)
    }

    /// Check If You Can Toggle The Border On Or Off
    pub fn is_border_toggle_supported() -> Result<bool, Box<dyn std::error::Error>> {
        Ok(ApiInformation::IsPropertyPresent(
            &HSTRING::from("Windows.Graphics.Capture.GraphicsCaptureSession"),
            &HSTRING::from("IsBorderRequired"),
        )?)
    }
}

impl Drop for GraphicsCaptureApi {
    fn drop(&mut self) {
        if !self.closed {
            if let Some(frame_pool) = self.frame_pool.take() {
                frame_pool.Close().expect("Failed to Close Frame Pool");
            }

            if let Some(session) = self.session.take() {
                session.Close().expect("Failed to Close Capture Session");
            }
        }

        unsafe { alloc::dealloc(self.buffer.ptr, self.buffer.layout) };
    }
}
