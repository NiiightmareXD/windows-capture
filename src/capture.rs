use std::{
    error::Error,
    os::windows::prelude::AsRawHandle,
    thread::{self, JoinHandle},
};

use log::{info, trace, warn};
use windows::{
    Foundation::AsyncActionCompletedHandler,
    Win32::{
        Foundation::{HANDLE, LPARAM, WPARAM},
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED, COINIT_SPEED_OVER_MEMORY},
            Threading::GetThreadId,
            WinRT::{
                CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE,
                DQTYPE_THREAD_CURRENT,
            },
        },
        UI::{
            HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE},
            WindowsAndMessaging::{
                DispatchMessageW, GetMessageW, PostQuitMessage, PostThreadMessageW,
                TranslateMessage, MSG, WM_QUIT,
            },
        },
    },
};

use crate::{
    frame::Frame,
    graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl, RESULT},
    settings::WindowsCaptureSettings,
};

/// Used To Handle Capture Control Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum CaptureControlError {
    #[error("Failed To Join Thread")]
    FailedToJoin,
}

/// Struct Used To Control Capture Thread
pub struct CaptureControl {
    thread_handle: Option<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>,
}

impl CaptureControl {
    /// Create A New Capture Control Struct
    #[must_use]
    pub fn new(thread_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>) -> Self {
        Self {
            thread_handle: Some(thread_handle),
        }
    }

    /// Wait Until The Thread Stops
    pub fn wait(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        if let Some(thread_handle) = self.thread_handle.take() {
            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoin));
                }
            }
        }

        Ok(())
    }

    /// Gracefully Stop The Capture Thread
    pub fn stop(mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
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

                        if e.code().0 == -2147023452 {
                            warn!("Thread Is Not In Message Loop Yet");
                        } else {
                            Err(e)?;
                        }
                    }
                }
            }

            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoin));
                }
            }
        }

        Ok(())
    }
}

/// Event Handler Trait
pub trait WindowsCaptureHandler: Sized {
    /// To Get The Message From The Settings
    type Flags;

    /// Starts The Capture And Takes Control Of The Current Thread
    fn start(
        settings: WindowsCaptureSettings<Self::Flags>,
    ) -> Result<(), Box<dyn Error + Send + Sync>>
    where
        Self: Send + 'static,
        <Self as WindowsCaptureHandler>::Flags: Send,
    {
        // Initialize COM
        trace!("Initializing COM");
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED | COINIT_SPEED_OVER_MEMORY)? };

        // Set DPI Awarness
        trace!("Setting DPI Awarness");
        unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE)? };

        // Create A Dispatcher Queue For Current Thread
        trace!("Creating A Dispatcher Queue For Capture Thread");
        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let controller = unsafe { CreateDispatcherQueueController(options)? };

        // Start Capture
        info!("Starting Capture Thread");
        let trigger = Self::new(settings.flags)?;
        let mut capture = GraphicsCaptureApi::new(
            settings.item,
            trigger,
            settings.capture_cursor,
            settings.draw_border,
            settings.color_format,
        )?;
        capture.start_capture()?;

        // Message Loop
        trace!("Entering Message Loop");
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Shutdown Dispatcher Queue
        trace!("Shutting Down Dispatcher Queue");
        let async_action = controller.ShutdownQueueAsync()?;
        async_action.SetCompleted(&AsyncActionCompletedHandler::new(
            move |_, _| -> Result<(), windows::core::Error> {
                unsafe { PostQuitMessage(0) };
                Ok(())
            },
        ))?;

        // Final Message Loop
        trace!("Entering Final Message Loop");
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Stop Capturing
        info!("Stopping Capture Thread");
        capture.stop_capture();

        // Uninitialize COM
        trace!("Uninitializing COM");
        unsafe { CoUninitialize() };

        // Check RESULT
        trace!("Checking RESULT");
        let result = RESULT.take().expect("Failed To Take RESULT");

        result?;

        Ok(())
    }

    /// Starts The Capture Without Taking Control Of The Current Thread
    fn start_free_threaded(settings: WindowsCaptureSettings<Self::Flags>) -> CaptureControl
    where
        Self: Send + 'static,
        <Self as WindowsCaptureHandler>::Flags: Send,
    {
        let thread_handle = thread::spawn(move || Self::start(settings));

        CaptureControl::new(thread_handle)
    }

    /// Function That Will Be Called To Create The Struct The Flags Can Be
    /// Passed From Settings
    fn new(flags: Self::Flags) -> Result<Self, Box<dyn Error + Send + Sync>>;

    /// Called Every Time A New Frame Is Available
    fn on_frame_arrived(
        &mut self,
        frame: Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Called When The Capture Item Closes Usually When The Window Closes,
    /// Capture Session Will End After This Function Ends
    fn on_closed(&mut self) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Call To Stop The Capture Thread, You Might Receive A Few More Frames
    /// Before It Stops
    fn stop(&self) {
        unsafe { PostQuitMessage(0) };
    }
}
