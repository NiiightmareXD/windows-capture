use std::{
    error::Error,
    mem,
    os::windows::prelude::AsRawHandle,
    sync::{
        atomic::{self, AtomicBool},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

use log::{debug, info, trace, warn};
use parking_lot::Mutex;
use windows::{
    Foundation::AsyncActionCompletedHandler,
    Win32::{
        Foundation::{HANDLE, LPARAM, WPARAM},
        System::{
            Threading::{GetCurrentThreadId, GetThreadId},
            WinRT::{
                CreateDispatcherQueueController, DispatcherQueueOptions, RoInitialize,
                RoUninitialize, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, RO_INIT_MULTITHREADED,
            },
        },
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, PostQuitMessage, PostThreadMessageW, TranslateMessage,
            MSG, WM_QUIT,
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
    FailedToJoinThread,
    #[error("Thread Handle Is Taken Out Of Struct")]
    ThreadHandleIsTaken,
}

/// Struct Used To Control Capture Thread
pub struct CaptureControl<T: WindowsCaptureHandler + Send + 'static> {
    thread_handle: Option<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>>,
    halt_handle: Arc<AtomicBool>,
    callback: Arc<Mutex<T>>,
}

impl<T: WindowsCaptureHandler + Send + 'static> CaptureControl<T> {
    /// Create A New Capture Control Struct
    #[must_use]
    pub fn new(
        thread_handle: JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>,
        halt_handle: Arc<AtomicBool>,
        callback: Arc<Mutex<T>>,
    ) -> Self {
        Self {
            thread_handle: Some(thread_handle),
            halt_handle,
            callback,
        }
    }

    /// Check To See If Capture Thread Is Finished
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread_handle
            .as_ref()
            .map_or(true, |thread_handle| thread_handle.is_finished())
    }

    /// Get The Halt Handle Used To Pause The Capture Thread
    #[must_use]
    pub fn into_thread_handle(self) -> JoinHandle<Result<(), Box<dyn Error + Send + Sync>>> {
        self.thread_handle.unwrap()
    }

    /// Get The Halt Handle Used To Pause The Capture Thread
    #[must_use]
    pub fn halt_handle(&self) -> Arc<AtomicBool> {
        self.halt_handle.clone()
    }

    /// Get The Callback Struct Used To Call Struct Methods Directly
    #[must_use]
    pub fn callback(&self) -> Arc<Mutex<T>> {
        self.callback.clone()
    }

    /// Wait Until The Capturing Thread Stops
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
                    return Err(Box::new(CaptureControlError::FailedToJoinThread));
                }
            }
        } else {
            return Err(Box::new(CaptureControlError::ThreadHandleIsTaken));
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
        // Initialize WinRT
        trace!("Initializing WinRT");
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

        // Create A Dispatcher Queue For Current Thread
        trace!("Creating A Dispatcher Queue For Capture Thread");
        let options = DispatcherQueueOptions {
            dwSize: mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let controller = unsafe { CreateDispatcherQueueController(options)? };

        // Start Capture
        info!("Starting Capture Thread");
        let callback = Arc::new(Mutex::new(Self::new(settings.flags)?));
        let mut capture = GraphicsCaptureApi::new(
            settings.item,
            callback,
            settings.capture_cursor,
            settings.draw_border,
            settings.color_format,
        )?;
        capture.start_capture()?;

        // Debug Thread ID
        debug!("Thread ID: {}", unsafe { GetCurrentThreadId() });

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

        // Check RESULT
        trace!("Checking RESULT");
        let result = RESULT.take().expect("Failed To Take RESULT");

        result?;

        Ok(())
    }

    /// Starts The Capture Without Taking Control Of The Current Thread
    fn start_free_threaded(
        settings: WindowsCaptureSettings<Self::Flags>,
    ) -> Result<CaptureControl<Self>, Box<dyn Error + Send + Sync>>
    where
        Self: Send + 'static,
        <Self as WindowsCaptureHandler>::Flags: Send,
    {
        let (halt_sender, halt_receiver) = mpsc::channel::<Arc<AtomicBool>>();
        let (callback_sender, callback_receiver) = mpsc::channel::<Arc<Mutex<Self>>>();

        let thread_handle = thread::spawn(move || -> Result<(), Box<dyn Error + Send + Sync>> {
            // Initialize WinRT
            trace!("Initializing WinRT");
            unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };

            // Create A Dispatcher Queue For Current Thread
            trace!("Creating A Dispatcher Queue For Capture Thread");
            let options = DispatcherQueueOptions {
                dwSize: mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            let controller = unsafe { CreateDispatcherQueueController(options)? };

            // Start Capture
            info!("Starting Capture Thread");
            let callback = Arc::new(Mutex::new(Self::new(settings.flags)?));
            let mut capture = GraphicsCaptureApi::new(
                settings.item,
                callback.clone(),
                settings.capture_cursor,
                settings.draw_border,
                settings.color_format,
            )?;
            capture.start_capture()?;

            // Send Halt Handle
            trace!("Sending Halt Handle");
            let halt_handle = capture.halt_handle();
            halt_sender.send(halt_handle)?;

            // Send Callback
            trace!("Sending Callback");
            callback_sender.send(callback)?;

            // Debug Thread ID
            debug!("Thread ID: {}", unsafe { GetCurrentThreadId() });

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

            // Check RESULT
            trace!("Checking RESULT");
            let result = RESULT.take().expect("Failed To Take RESULT");

            result?;

            Ok(())
        });

        let halt_handle = match halt_receiver.recv() {
            Ok(halt_handle) => halt_handle,
            Err(_) => match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoinThread));
                }
            },
        };

        let callback = match callback_receiver.recv() {
            Ok(callback_handle) => callback_handle,
            Err(_) => match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(Box::new(CaptureControlError::FailedToJoinThread));
                }
            },
        };

        Ok(CaptureControl::new(thread_handle, halt_handle, callback))
    }

    /// Function That Will Be Called To Create The Struct The Flags Can Be
    /// Passed From Settings
    fn new(flags: Self::Flags) -> Result<Self, Box<dyn Error + Send + Sync>>;

    /// Called Every Time A New Frame Is Available
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Called When The Capture Item Closes Usually When The Window Closes,
    /// Capture Session Will End After This Function Ends
    fn on_closed(&mut self) -> Result<(), Box<dyn Error + Send + Sync>>;
}
