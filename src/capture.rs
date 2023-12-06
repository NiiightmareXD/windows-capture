use std::{
    mem,
    os::windows::prelude::AsRawHandle,
    sync::{
        atomic::{self, AtomicBool},
        mpsc::{self, SendError},
        Arc,
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
    graphics_capture_api::{self, GraphicsCaptureApi, InternalCaptureControl},
    settings::Settings,
};

/// Used To Handle Capture Control Errors
#[derive(thiserror::Error, Debug)]
#[allow(clippy::module_name_repetitions)]
pub enum CaptureControlError<E> {
    #[error("Failed To Join Thread")]
    FailedToJoinThread,
    #[error("Thread Handle Is Taken Out Of Struct")]
    ThreadHandleIsTaken,
    #[error("Failed To Post Thread Message")]
    FailedToPostThreadMessage,
    #[error(transparent)]
    WindowsCaptureError(#[from] WindowsCaptureError<E>),
}

/// Struct Used To Control Capture Thread
#[allow(clippy::module_name_repetitions)]
pub struct CaptureControl<T: WindowsCaptureHandler + Send + 'static + ?Sized, E> {
    thread_handle: Option<JoinHandle<Result<(), WindowsCaptureError<E>>>>,
    halt_handle: Arc<AtomicBool>,
    callback: Arc<Mutex<T>>,
}

impl<T: WindowsCaptureHandler + Send + 'static, E> CaptureControl<T, E> {
    /// Create A New Capture Control Struct
    #[must_use]
    pub fn new(
        thread_handle: JoinHandle<Result<(), WindowsCaptureError<E>>>,
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
            .map_or(true, std::thread::JoinHandle::is_finished)
    }

    /// Get The Halt Handle Used To Pause The Capture Thread
    #[must_use]
    pub fn into_thread_handle(self) -> JoinHandle<Result<(), WindowsCaptureError<E>>> {
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
    pub fn wait(mut self) -> Result<(), CaptureControlError<E>> {
        if let Some(thread_handle) = self.thread_handle.take() {
            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(CaptureControlError::FailedToJoinThread);
                }
            }
        } else {
            return Err(CaptureControlError::ThreadHandleIsTaken);
        }

        Ok(())
    }

    /// Gracefully Stop The Capture Thread
    pub fn stop(mut self) -> Result<(), CaptureControlError<E>> {
        self.halt_handle.store(true, atomic::Ordering::Relaxed);

        if let Some(thread_handle) = self.thread_handle.take() {
            let handle = thread_handle.as_raw_handle();
            let handle = HANDLE(handle as isize);
            let therad_id = unsafe { GetThreadId(handle) };

            loop {
                match unsafe {
                    PostThreadMessageW(therad_id, WM_QUIT, WPARAM::default(), LPARAM::default())
                } {
                    Ok(()) => break,
                    Err(e) => {
                        if thread_handle.is_finished() {
                            break;
                        }

                        if e.code().0 == -2_147_023_452 {
                            warn!("Thread Is Not In Message Loop Yet");
                        } else {
                            Err(e).map_err(|_| CaptureControlError::FailedToPostThreadMessage)?;
                        }
                    }
                }
            }

            match thread_handle.join() {
                Ok(result) => result?,
                Err(_) => {
                    return Err(CaptureControlError::FailedToJoinThread);
                }
            }
        } else {
            return Err(CaptureControlError::ThreadHandleIsTaken);
        }

        Ok(())
    }
}

/// Used To Handle Capture Control Errors
#[derive(thiserror::Error, Debug)]
pub enum WindowsCaptureError<E> {
    #[error("Failed To Join Thread")]
    FailedToJoinThread,
    #[error("Failed To Initialize WinRT")]
    FailedToInitWinRT,
    #[error("Failed To Create Dispatcher Queue Controller")]
    FailedToCreateDispatcherQueueController,
    #[error("Failed To Shutdown Dispatcher Queue")]
    FailedToShutdownDispatcherQueue,
    #[error("Failed To Set Dispatcher Queue Completed Handler")]
    FailedToSetDispatcherQueueCompletedHandler,
    #[error("Graphics Capture Error")]
    GraphicsCaptureError(graphics_capture_api::Error),
    #[error("Handler Error")]
    HandlerError(E),
    #[error(transparent)]
    FailedToThreadID(SendError<u32>),
}

/// Event Handler Trait
pub trait WindowsCaptureHandler: Sized {
    /// To Get The Message From The Settings
    type Flags;

    /// To Redirect To `CaptureControl` Or `Start` Method
    type Error: Send + Sync;

    /// Starts The Capture And Takes Control Of The Current Thread
    fn start(settings: Settings<Self::Flags>) -> Result<(), WindowsCaptureError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as WindowsCaptureHandler>::Flags: Send,
    {
        // Initialize WinRT
        trace!("Initializing WinRT");
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)
                .map_err(|_| WindowsCaptureError::FailedToInitWinRT)?;
        };

        // Create A Dispatcher Queue For Current Thread
        trace!("Creating A Dispatcher Queue For Capture Thread");
        let options = DispatcherQueueOptions {
            dwSize: u32::try_from(mem::size_of::<DispatcherQueueOptions>()).unwrap(),
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let controller = unsafe {
            CreateDispatcherQueueController(options)
                .map_err(|_| WindowsCaptureError::FailedToCreateDispatcherQueueController)?
        };

        // Debug Thread ID
        let thread_id = unsafe { GetCurrentThreadId() };
        debug!("Thread ID: {thread_id}");

        // Start Capture
        info!("Starting Capture Thread");
        let result = Arc::new(Mutex::new(None));
        let callback = Arc::new(Mutex::new(
            Self::new(settings.flags).map_err(WindowsCaptureError::HandlerError)?,
        ));
        let mut capture = GraphicsCaptureApi::new(
            settings.item,
            callback,
            settings.capture_cursor,
            settings.draw_border,
            settings.color_format,
            thread_id,
            result.clone(),
        )
        .map_err(WindowsCaptureError::GraphicsCaptureError)?;
        capture
            .start_capture()
            .map_err(WindowsCaptureError::GraphicsCaptureError)?;

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
        let async_action = controller
            .ShutdownQueueAsync()
            .map_err(|_| WindowsCaptureError::FailedToShutdownDispatcherQueue)?;
        async_action
            .SetCompleted(&AsyncActionCompletedHandler::new(
                move |_, _| -> Result<(), windows::core::Error> {
                    unsafe { PostQuitMessage(0) };
                    Ok(())
                },
            ))
            .map_err(|_| WindowsCaptureError::FailedToSetDispatcherQueueCompletedHandler)?;

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

        // Check Handler Result
        trace!("Checking Handler Result");
        if let Some(e) = result.lock().take() {
            return Err(WindowsCaptureError::HandlerError(e));
        }

        Ok(())
    }

    /// Starts The Capture Without Taking Control Of The Current Thread
    #[allow(clippy::too_many_lines)]
    fn start_free_threaded(
        settings: Settings<Self::Flags>,
    ) -> Result<CaptureControl<Self, Self::Error>, WindowsCaptureError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as WindowsCaptureHandler>::Flags: Send,
    {
        let (halt_sender, halt_receiver) = mpsc::channel::<Arc<AtomicBool>>();
        let (callback_sender, callback_receiver) = mpsc::channel::<Arc<Mutex<Self>>>();

        let thread_handle =
            thread::spawn(move || -> Result<(), WindowsCaptureError<Self::Error>> {
                // Initialize WinRT
                trace!("Initializing WinRT");
                unsafe {
                    RoInitialize(RO_INIT_MULTITHREADED)
                        .map_err(|_| WindowsCaptureError::FailedToInitWinRT)?;
                };

                // Create A Dispatcher Queue For Current Thread
                trace!("Creating A Dispatcher Queue For Capture Thread");
                let options = DispatcherQueueOptions {
                    dwSize: u32::try_from(mem::size_of::<DispatcherQueueOptions>()).unwrap(),
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_NONE,
                };
                let controller = unsafe {
                    CreateDispatcherQueueController(options)
                        .map_err(|_| WindowsCaptureError::FailedToCreateDispatcherQueueController)?
                };

                // Debug Thread ID
                let thread_id = unsafe { GetCurrentThreadId() };
                debug!("Thread ID: {thread_id}");

                // Start Capture
                info!("Starting Capture Thread");
                let result = Arc::new(Mutex::new(None));
                let callback = Arc::new(Mutex::new(
                    Self::new(settings.flags).map_err(WindowsCaptureError::HandlerError)?,
                ));
                let mut capture = GraphicsCaptureApi::new(
                    settings.item,
                    callback.clone(),
                    settings.capture_cursor,
                    settings.draw_border,
                    settings.color_format,
                    thread_id,
                    result.clone(),
                )
                .map_err(WindowsCaptureError::GraphicsCaptureError)?;
                capture
                    .start_capture()
                    .map_err(WindowsCaptureError::GraphicsCaptureError)?;

                // Send Halt Handle
                trace!("Sending Halt Handle");
                let halt_handle = capture.halt_handle();
                halt_sender.send(halt_handle).unwrap();

                // Send Callback
                trace!("Sending Callback");
                callback_sender.send(callback).unwrap();

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
                let async_action = controller
                    .ShutdownQueueAsync()
                    .map_err(|_| WindowsCaptureError::FailedToShutdownDispatcherQueue)?;

                async_action
                    .SetCompleted(&AsyncActionCompletedHandler::new(
                        move |_, _| -> Result<(), windows::core::Error> {
                            unsafe { PostQuitMessage(0) };
                            Ok(())
                        },
                    ))
                    .map_err(|_| WindowsCaptureError::FailedToSetDispatcherQueueCompletedHandler)?;

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

                // Check Handler Result
                trace!("Checking Handler Result");
                if let Some(e) = result.lock().take() {
                    return Err(WindowsCaptureError::HandlerError(e));
                }

                Ok(())
            });

        let Ok(halt_handle) = halt_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(WindowsCaptureError::FailedToJoinThread);
                }
            }
        };

        let Ok(callback) = callback_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(WindowsCaptureError::FailedToJoinThread);
                }
            }
        };

        Ok(CaptureControl::new(thread_handle, halt_handle, callback))
    }

    /// Function That Will Be Called To Create The Struct The Flags Can Be
    /// Passed From Settings
    fn new(flags: Self::Flags) -> Result<Self, Self::Error>;

    /// Called Every Time A New Frame Is Available
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error>;

    /// Called When The Capture Item Closes Usually When The Window Closes,
    /// Capture Session Will End After This Function Ends
    fn on_closed(&mut self) -> Result<(), Self::Error>;
}
