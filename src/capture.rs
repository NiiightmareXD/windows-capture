use log::{info, trace};
use windows::{
    Foundation::AsyncActionCompletedHandler,
    Win32::{
        System::WinRT::{
            CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize, RoUninitialize,
            DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_SINGLETHREADED,
        },
        UI::{
            HiDpi::{SetProcessDpiAwareness, PROCESS_PER_MONITOR_DPI_AWARE},
            WindowsAndMessaging::{
                DispatchMessageW, GetMessageW, PostQuitMessage, TranslateMessage, MSG,
            },
        },
    },
};

use crate::{
    frame::Frame, graphics_capture_api::GraphicsCaptureApi, settings::WindowsCaptureSettings,
};

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
        // Initialize WinRT
        trace!("Initializing WinRT");
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED)? };

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
        let trigger = Self::new(settings.flags);
        let mut capture = GraphicsCaptureApi::new(settings.item, trigger)?;
        capture.start_capture(settings.capture_cursor, settings.draw_border)?;

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

        // Uninitialize WinRT
        trace!("Uninitializing WinRT");
        unsafe { RoUninitialize() };

        Ok(())
    }

    /// Function That Will Be Called To Create The Struct The Flags Can Be
    /// Passed From Settings
    fn new(flags: Self::Flags) -> Self;

    /// Called Every Time A New Frame Is Available
    fn on_frame_arrived(&mut self, frame: Frame);

    /// Called When The Capture Item Closes Usually When The Window Closes,
    /// Capture Will End After This Function Ends
    fn on_closed(&mut self);

    /// Call To Stop The Capture Thread, You Might Receive A Few More Frames
    /// Before It Stops
    fn stop(&self) {
        unsafe { PostQuitMessage(0) };
    }
}
