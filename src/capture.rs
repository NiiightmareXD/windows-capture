use std::{
    mem,
    os::windows::prelude::AsRawHandle,
    sync::{
        atomic::{self, AtomicBool},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

use parking_lot::Mutex;
use windows::{
    Foundation::AsyncActionCompletedHandler,
    Graphics::Capture::GraphicsCaptureItem,
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

#[derive(thiserror::Error, Debug)]
pub enum CaptureControlError<E> {
    #[error("Failed to join thread")]
    FailedToJoinThread,
    #[error("Thread handle is taken out of struct")]
    ThreadHandleIsTaken,
    #[error("Failed to post thread message")]
    FailedToPostThreadMessage,
    #[error("Stopped handler error: {0}")]
    StoppedHandlerError(E),
    #[error("Windows capture error: {0}")]
    GraphicsCaptureApiError(#[from] GraphicsCaptureApiError<E>),
}

/// Used to control the capture session
pub struct CaptureControl<T: GraphicsCaptureApiHandler + Send + 'static + ?Sized, E> {
    thread_handle: Option<JoinHandle<Result<(), GraphicsCaptureApiError<E>>>>,
    halt_handle: Arc<AtomicBool>,
    callback: Arc<Mutex<T>>,
}

impl<T: GraphicsCaptureApiHandler + Send + 'static, E> CaptureControl<T, E> {
    /// Creates a new Capture Control struct.
    ///
    /// # Arguments
    ///
    /// * `thread_handle` - The join handle for the capture thread.
    /// * `halt_handle` - The atomic boolean used to pause the capture thread.
    /// * `callback` - The mutex-protected callback struct used to call struct methods directly.
    ///
    /// # Returns
    ///
    /// The newly created CaptureControl struct.
    #[must_use]
    #[inline]
    pub fn new(
        thread_handle: JoinHandle<Result<(), GraphicsCaptureApiError<E>>>,
        halt_handle: Arc<AtomicBool>,
        callback: Arc<Mutex<T>>,
    ) -> Self {
        Self {
            thread_handle: Some(thread_handle),
            halt_handle,
            callback,
        }
    }

    /// Checks to see if the capture thread is finished.
    ///
    /// # Returns
    ///
    /// `true` if the capture thread is finished, `false` otherwise.
    #[must_use]
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.thread_handle
            .as_ref()
            .map_or(true, std::thread::JoinHandle::is_finished)
    }

    /// Gets the join handle for the capture thread.
    ///
    /// # Returns
    ///
    /// The join handle for the capture thread.
    #[must_use]
    #[inline]
    pub fn into_thread_handle(self) -> JoinHandle<Result<(), GraphicsCaptureApiError<E>>> {
        self.thread_handle.unwrap()
    }

    /// Gets the halt handle used to pause the capture thread.
    ///
    /// # Returns
    ///
    /// The halt handle used to pause the capture thread.
    #[must_use]
    #[inline]
    pub fn halt_handle(&self) -> Arc<AtomicBool> {
        self.halt_handle.clone()
    }

    /// Gets the callback struct used to call struct methods directly.
    ///
    /// # Returns
    ///
    /// The callback struct used to call struct methods directly.
    #[must_use]
    #[inline]
    pub fn callback(&self) -> Arc<Mutex<T>> {
        self.callback.clone()
    }

    /// Waits until the capturing thread stops.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the capturing thread stops successfully, an error otherwise.
    #[inline]
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

    /// Gracefully stops the capture thread.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the capture thread stops successfully, an error otherwise.
    #[inline]
    pub fn stop(mut self) -> Result<(), CaptureControlError<E>> {
        self.halt_handle.store(true, atomic::Ordering::Relaxed);

        if let Some(thread_handle) = self.thread_handle.take() {
            let handle = thread_handle.as_raw_handle();
            let handle = HANDLE(handle);
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

                        if e.code().0 != -2_147_023_452 {
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

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
pub enum GraphicsCaptureApiError<E> {
    #[error("Failed to join thread")]
    FailedToJoinThread,
    #[error("Failed to initialize WinRT")]
    FailedToInitWinRT,
    #[error("Failed to create dispatcher queue controller")]
    FailedToCreateDispatcherQueueController,
    #[error("Failed to shutdown dispatcher queue")]
    FailedToShutdownDispatcherQueue,
    #[error("Failed to set dispatcher queue completed handler")]
    FailedToSetDispatcherQueueCompletedHandler,
    #[error("Failed to convert item to GraphicsCaptureItem")]
    ItemConvertFailed,
    #[error("Graphics capture error: {0}")]
    GraphicsCaptureApiError(graphics_capture_api::Error),
    #[error("New handler error: {0}")]
    NewHandlerError(E),
    #[error("Frame handler error: {0}")]
    FrameHandlerError(E),
}

/// A trait representing a graphics capture handler.

pub trait GraphicsCaptureApiHandler: Sized {
    /// The type of flags used to get the values from the settings.
    type Flags;

    /// The type of error that can occur during capture, the error will be returned from `CaptureControl` and `start` functions.
    type Error: Send + Sync;

    /// Starts the capture and takes control of the current thread.
    ///
    /// # Arguments
    ///
    /// * `settings` - The capture settings.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the capture was successful, otherwise returns an error of type `GraphicsCaptureApiError`.
    #[inline]
    fn start<T: TryInto<GraphicsCaptureItem>>(
        settings: Settings<Self::Flags, T>,
    ) -> Result<(), GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send,
    {
        // Initialize WinRT
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED)
                .map_err(|_| GraphicsCaptureApiError::FailedToInitWinRT)?;
        };

        // Create a dispatcher queue for the current thread
        let options = DispatcherQueueOptions {
            dwSize: u32::try_from(mem::size_of::<DispatcherQueueOptions>()).unwrap(),
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let controller = unsafe {
            CreateDispatcherQueueController(options)
                .map_err(|_| GraphicsCaptureApiError::FailedToCreateDispatcherQueueController)?
        };

        // Get current thread ID
        let thread_id = unsafe { GetCurrentThreadId() };

        // Start capture
        let result = Arc::new(Mutex::new(None));
        let callback = Arc::new(Mutex::new(
            Self::new(settings.flags).map_err(GraphicsCaptureApiError::NewHandlerError)?,
        ));

        let item = settings
            .item
            .try_into()
            .map_err(|_| GraphicsCaptureApiError::ItemConvertFailed)?;

        let mut capture = GraphicsCaptureApi::new(
            item,
            callback,
            settings.cursor_capture,
            settings.draw_border,
            settings.color_format,
            thread_id,
            result.clone(),
        )
        .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;
        capture
            .start_capture()
            .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

        // Message loop
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Shutdown dispatcher queue
        let async_action = controller
            .ShutdownQueueAsync()
            .map_err(|_| GraphicsCaptureApiError::FailedToShutdownDispatcherQueue)?;
        async_action
            .SetCompleted(&AsyncActionCompletedHandler::new(
                move |_, _| -> Result<(), windows::core::Error> {
                    unsafe { PostQuitMessage(0) };
                    Ok(())
                },
            ))
            .map_err(|_| GraphicsCaptureApiError::FailedToSetDispatcherQueueCompletedHandler)?;

        // Final message loop
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Stop capture
        capture.stop_capture();

        // Uninitialize WinRT
        unsafe { RoUninitialize() };

        // Check handler result
        if let Some(e) = result.lock().take() {
            return Err(GraphicsCaptureApiError::FrameHandlerError(e));
        }

        Ok(())
    }

    /// Starts the capture without taking control of the current thread.
    ///
    /// # Arguments
    ///
    /// * `settings` - The capture settings.
    ///
    /// # Returns
    ///
    /// Returns `Ok(CaptureControl)` if the capture was successful, otherwise returns an error of type `GraphicsCaptureApiError`.
    #[inline]
    fn start_free_threaded<T: TryInto<GraphicsCaptureItem> + Send + 'static>(
        settings: Settings<Self::Flags, T>,
    ) -> Result<CaptureControl<Self, Self::Error>, GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send,
    {
        let (halt_sender, halt_receiver) = mpsc::channel::<Arc<AtomicBool>>();
        let (callback_sender, callback_receiver) = mpsc::channel::<Arc<Mutex<Self>>>();

        let thread_handle = thread::spawn(
            move || -> Result<(), GraphicsCaptureApiError<Self::Error>> {
                // Initialize WinRT
                unsafe {
                    RoInitialize(RO_INIT_MULTITHREADED)
                        .map_err(|_| GraphicsCaptureApiError::FailedToInitWinRT)?;
                };

                // Create a dispatcher queue for the current thread
                let options = DispatcherQueueOptions {
                    dwSize: u32::try_from(mem::size_of::<DispatcherQueueOptions>()).unwrap(),
                    threadType: DQTYPE_THREAD_CURRENT,
                    apartmentType: DQTAT_COM_NONE,
                };
                let controller = unsafe {
                    CreateDispatcherQueueController(options).map_err(|_| {
                        GraphicsCaptureApiError::FailedToCreateDispatcherQueueController
                    })?
                };

                // Get current thread ID
                let thread_id = unsafe { GetCurrentThreadId() };

                // Start capture
                let result = Arc::new(Mutex::new(None));
                let callback = Arc::new(Mutex::new(
                    Self::new(settings.flags).map_err(GraphicsCaptureApiError::NewHandlerError)?,
                ));

                let item = settings
                    .item
                    .try_into()
                    .map_err(|_| GraphicsCaptureApiError::ItemConvertFailed)?;

                let mut capture = GraphicsCaptureApi::new(
                    item,
                    callback.clone(),
                    settings.cursor_capture,
                    settings.draw_border,
                    settings.color_format,
                    thread_id,
                    result.clone(),
                )
                .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;
                capture
                    .start_capture()
                    .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

                // Send halt handle
                let halt_handle = capture.halt_handle();
                halt_sender.send(halt_handle).unwrap();

                // Send callback
                callback_sender.send(callback).unwrap();

                // Message loop
                let mut message = MSG::default();
                unsafe {
                    while GetMessageW(&mut message, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }

                // Shutdown dispatcher queue
                let async_action = controller
                    .ShutdownQueueAsync()
                    .map_err(|_| GraphicsCaptureApiError::FailedToShutdownDispatcherQueue)?;

                async_action
                    .SetCompleted(&AsyncActionCompletedHandler::new(
                        move |_, _| -> Result<(), windows::core::Error> {
                            unsafe { PostQuitMessage(0) };
                            Ok(())
                        },
                    ))
                    .map_err(|_| {
                        GraphicsCaptureApiError::FailedToSetDispatcherQueueCompletedHandler
                    })?;

                // Final message loop
                let mut message = MSG::default();
                unsafe {
                    while GetMessageW(&mut message, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }

                // Stop capture
                capture.stop_capture();

                // Uninitialize WinRT
                unsafe { RoUninitialize() };

                // Check handler result
                if let Some(e) = result.lock().take() {
                    return Err(GraphicsCaptureApiError::FrameHandlerError(e));
                }

                Ok(())
            },
        );

        let Ok(halt_handle) = halt_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(GraphicsCaptureApiError::FailedToJoinThread);
                }
            }
        };

        let Ok(callback) = callback_receiver.recv() else {
            match thread_handle.join() {
                Ok(result) => return Err(result.err().unwrap()),
                Err(_) => {
                    return Err(GraphicsCaptureApiError::FailedToJoinThread);
                }
            }
        };

        Ok(CaptureControl::new(thread_handle, halt_handle, callback))
    }

    /// Function that will be called to create the struct. The flags can be passed from settings.
    ///
    /// # Arguments
    ///
    /// * `flags` - The flags used to create the struct.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Self)` if the struct creation was successful, otherwise returns an error of type `Self::Error`.
    fn new(flags: Self::Flags) -> Result<Self, Self::Error>;

    /// Called every time a new frame is available.
    ///
    /// # Arguments
    ///
    /// * `frame` - A mutable reference to the captured frame.
    /// * `capture_control` - The internal capture control.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame processing was successful, otherwise returns an error of type `Self::Error`.
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error>;

    /// Optional handler called when the capture item (usually a window) closes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the handler execution was successful, otherwise returns an error of type `Self::Error`.
    #[inline]
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
