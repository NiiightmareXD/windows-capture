use std::sync::Arc;

use log::{info, warn};
use parking_lot::Mutex;
use thiserror::Error;
use windows::{
    core::{ComInterface, IInspectable},
    Foundation::{AsyncActionCompletedHandler, TypedEventHandler},
    Graphics::{
        Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession},
        DirectX::DirectXPixelFormat,
    },
    Win32::{
        Graphics::{
            Direct3D11::{
                ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
            },
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        },
        System::WinRT::{
            CreateDispatcherQueueController, Direct3D11::IDirect3DDxgiInterfaceAccess,
            DispatcherQueueOptions, RoInitialize, RoUninitialize, DQTAT_COM_NONE,
            DQTYPE_THREAD_CURRENT, RO_INIT_MULTITHREADED,
        },
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, PostQuitMessage, TranslateMessage, MSG,
        },
    },
};

use crate::{d3d11::SendDirectX, monitor::Monitor};

use super::{
    d3d11::{create_d3d_device, create_direct3d_device},
    frame::Frame,
};

/// Used To Handle Internal Errors
#[derive(Error, Debug)]
pub enum WindowsCaptureError {
    #[error("Graphics Capture API Is Not Supported")]
    Unsupported,
    #[error("Already Started")]
    AlreadyStarted,
    #[error("Unknown Error")]
    Unknown,
}

/// Capture Settings
pub struct WindowsCaptureSettings<Flags> {
    /// Item That Can Be Created From Monitor Or Window
    pub item: GraphicsCaptureItem,
    /// Capture Mouse Cursor
    pub capture_cursor: bool,
    /// Draw Yellow Border Around Captured Window
    pub draw_border: bool,
    /// Flags To Pass To The New Function
    pub flags: Flags,
}

impl<Flags> Default for WindowsCaptureSettings<Flags>
where
    Flags: Default,
{
    fn default() -> Self {
        Self {
            item: Monitor::get_primary().into(),
            capture_cursor: false,
            draw_border: false,
            flags: Default::default(),
        }
    }
}

/// Internal Capture Struct
pub struct WindowsCapture {
    frame_pool: Option<Arc<Direct3D11CaptureFramePool>>,
    session: Option<GraphicsCaptureSession>,
    started: bool,
}

impl WindowsCapture {
    pub fn new<T: WindowsCaptureHandler + std::marker::Send + 'static>(
        _item: GraphicsCaptureItem,
        trigger: T,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Check Support
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(Box::new(WindowsCaptureError::Unsupported));
        }

        // Create Device
        let d3d_device = create_d3d_device()?;
        let device = create_direct3d_device(&d3d_device)?;

        // Create Frame Pool
        let frame_pool = Direct3D11CaptureFramePool::Create(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            _item.Size()?,
        )?;

        // Init
        let session = frame_pool.CreateCaptureSession(&_item)?;
        let trigger = Arc::new(Mutex::new(trigger));
        let trigger_item = trigger.clone();
        let trigger_frame_pool = trigger;
        let frame_pool = Arc::new(frame_pool);
        let device = SendDirectX::new(device);

        // Set CaptureItem Closed Event
        _item.Closed(
            &TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new({
                move |_, _| {
                    trigger_item.lock().on_closed();

                    unsafe { PostQuitMessage(0) };

                    Result::Ok(())
                }
            }),
        )?;

        // Set FramePool FrameArrived Event
        frame_pool.FrameArrived(
            &TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new({
                let context = unsafe { d3d_device.GetImmediateContext()? };
                let frame_pool = frame_pool.clone();
                let mut last_size = _item.Size()?;

                move |frame, _| {
                    // Get Frame
                    let frame = frame.as_ref().unwrap();
                    let frame = frame.TryGetNextFrame()?;

                    // Get Frame Content Size
                    let frame_content_size = frame.ContentSize()?;

                    // Get Frame Surface
                    let surface = frame.Surface()?;

                    // Convert Surface To Texture
                    let access = surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
                    let texture = unsafe { access.GetInterface::<ID3D11Texture2D>()? };

                    // Texture Settings
                    let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
                    unsafe { texture.GetDesc(&mut texture_desc) }
                    texture_desc.Usage = D3D11_USAGE_STAGING;
                    texture_desc.BindFlags = 0;
                    texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                    texture_desc.MiscFlags = 0;

                    if texture_desc.Format == DXGI_FORMAT_B8G8R8A8_UNORM {
                        // Check If The Size Has Been Changed
                        if frame_content_size.Width != last_size.Width
                            || frame_content_size.Height != last_size.Height
                        {
                            info!("Size Changed Recreating Device");
                            let device = &device;
                            frame_pool
                                .Recreate(
                                    &device.inner,
                                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                                    2,
                                    frame_content_size,
                                )
                                .unwrap();

                            last_size = frame_content_size;
                        } else {
                            let frame = Frame::new(&surface, &d3d_device, &context);

                            // Send The Frame To Trigger Struct
                            trigger_frame_pool.lock().on_frame_arrived(&frame);
                        }
                    } else {
                        warn!("Wrong Pixel Type");
                    }

                    Result::Ok(())
                }
            }),
        )?;

        Ok(Self {
            frame_pool: Some(frame_pool),
            session: Some(session),
            started: false,
        })
    }

    pub fn start_capture(
        &mut self,
        capture_cursor: bool,
        draw_border: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.started {
            return Err(Box::new(WindowsCaptureError::AlreadyStarted));
        }

        // Config
        self.session
            .as_ref()
            .unwrap()
            .SetIsCursorCaptureEnabled(capture_cursor)?;
        self.session
            .as_ref()
            .unwrap()
            .SetIsBorderRequired(draw_border)?;
        self.started = true;

        // Start Capture
        self.session.as_ref().unwrap().StartCapture()?;

        Ok(())
    }

    pub fn stop_capture(mut self) {
        // Stop Capturing
        if let Some(frame_pool) = self.frame_pool.take() {
            frame_pool.Close().expect("Failed to Close Frame Pool");
        }

        if let Some(session) = self.session.take() {
            session.Close().expect("Failed to Close Frame Pool");
        }
    }
}

impl Drop for WindowsCapture {
    fn drop(&mut self) {
        // Stop Capturing
        if let Some(frame_pool) = self.frame_pool.take() {
            frame_pool.Close().expect("Failed to Close Frame Pool");
        }

        if let Some(session) = self.session.take() {
            session.Close().expect("Failed to Close Frame Pool");
        }
    }
}

/// Event Handler Trait
pub trait WindowsCaptureHandler: Sized {
    type Flags;

    /// Starts The Capture And Takes Control Of The Current Thread
    fn start(
        settings: WindowsCaptureSettings<Self::Flags>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        Self: std::marker::Send + 'static,
    {
        // Init WinRT
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

        // Create A Dispatcher Queue For Current Thread
        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let controller = unsafe { CreateDispatcherQueueController(options)? };

        let trigger = Self::new(settings.flags);
        let mut capture = WindowsCapture::new(settings.item, trigger)?;
        capture.start_capture(settings.capture_cursor, settings.draw_border)?;

        // Message Loop
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).into() {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Shutdown Dispatcher Queue
        let async_action = controller.ShutdownQueueAsync()?;
        async_action.SetCompleted(&AsyncActionCompletedHandler::new(
            move |_, _| -> windows::core::Result<()> {
                unsafe { PostQuitMessage(0) };
                Ok(())
            },
        ))?;

        // Message Loop
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Stop Capturing
        capture.stop_capture();

        // Uninit WinRT
        unsafe { RoUninitialize() };

        Ok(())
    }

    /// Function That Will Be Called To Create The Struct The Flags Can Be
    /// Passed From Settigns
    fn new(flags: Self::Flags) -> Self;

    /// Called Every Time A New Frame Is Available
    fn on_frame_arrived(&mut self, frame: &Frame);

    /// Called If The Capture Item Closed Usually When The Window Closes
    fn on_closed(&mut self);
}
