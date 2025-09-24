use std::mem;
use std::os::windows::prelude::AsRawHandle;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread::{self, JoinHandle};

use parking_lot::Mutex;
use windows::Win32::Foundation::{HANDLE, LPARAM, S_FALSE, WPARAM};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::Threading::{GetCurrentThreadId, GetThreadId};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
    RO_INIT_MULTITHREADED, RoInitialize,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, PostThreadMessageW, TranslateMessage, WM_QUIT,
};
use windows::core::Result as WindowsResult;
use windows_future::AsyncActionCompletedHandler;

use crate::d3d11::{self, create_d3d_device};
use crate::frame::Frame;
use crate::graphics_capture_api::{self, GraphicsCaptureApi, InternalCaptureControl};
use crate::settings::{Settings, TryIntoCaptureItemWithDetails};

#[derive(thiserror::Error, Debug)]
/// Errors that can occur while controlling a running capture session via [`CaptureControl`].
///
/// This error wraps lower-level errors from the Windows Graphics Capture pipeline, as well as
/// thread-control failures when starting/stopping the background capture thread.
pub enum CaptureControlError<E> {
    /// Joining the background capture thread failed (panic or OS-level join error).
    ///
    /// Returned by [`CaptureControl::wait`] and [`CaptureControl::stop`] if the internal thread
    /// panicked or could not be joined.
    #[error("Failed to join thread")]
    FailedToJoinThread,
    /// The [`std::thread::JoinHandle`] was already taken out of the struct (for example by calling
    /// [`CaptureControl::into_thread_handle`]) so the operation cannot proceed.
    #[error("Thread handle is taken out of the struct")]
    ThreadHandleIsTaken,
    /// Failed to post a WM_QUIT message to the capture thread to request shutdown.
    ///
    /// This can happen if the thread is no longer alive or Windows refuses the message.
    #[error("Failed to post thread message")]
    FailedToPostThreadMessage,
    /// The user-provided handler returned an error after capture stopped.
    ///
    /// This variant carries the handler's error type.
    #[error("Stopped handler error: {0}")]
    StoppedHandlerError(E),
    /// A lower-level error from the graphics capture pipeline.
    ///
    /// Wraps [`GraphicsCaptureApiError`].
    #[error("Windows capture error: {0}")]
    GraphicsCaptureApiError(#[from] GraphicsCaptureApiError<E>),
}

/// Used to control the capture session
pub struct CaptureControl<T: GraphicsCaptureApiHandler + Send + 'static, E> {
    thread_handle: Option<JoinHandle<Result<(), GraphicsCaptureApiError<E>>>>,
    halt_handle: Arc<AtomicBool>,
    callback: Arc<Mutex<T>>,
}

impl<T: GraphicsCaptureApiHandler + Send + 'static, E> CaptureControl<T, E> {
    /// Constructs a new [`CaptureControl`].
    #[inline]
    #[must_use]
    pub const fn new(
        thread_handle: JoinHandle<Result<(), GraphicsCaptureApiError<E>>>,
        halt_handle: Arc<AtomicBool>,
        callback: Arc<Mutex<T>>,
    ) -> Self {
        Self { thread_handle: Some(thread_handle), halt_handle, callback }
    }

    /// Checks whether the capture thread has finished.
    #[inline]
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.thread_handle.as_ref().is_none_or(std::thread::JoinHandle::is_finished)
    }

    /// Gets the join handle for the capture thread.
    #[inline]
    #[must_use]
    pub fn into_thread_handle(self) -> JoinHandle<Result<(), GraphicsCaptureApiError<E>>> {
        self.thread_handle.unwrap()
    }

    /// Gets the halt handle used to pause the capture thread.
    #[inline]
    #[must_use]
    pub fn halt_handle(&self) -> Arc<AtomicBool> {
        self.halt_handle.clone()
    }

    /// Gets the callback struct used to call struct methods directly.
    #[inline]
    #[must_use]
    pub fn callback(&self) -> Arc<Mutex<T>> {
        self.callback.clone()
    }

    /// Waits for the capture thread to stop.
    ///
    /// # Errors
    ///
    /// - [`CaptureControlError::FailedToJoinThread`] when joining the internal thread fails
    /// - [`CaptureControlError::ThreadHandleIsTaken`] when the thread handle was previously taken
    ///   via [`CaptureControl::into_thread_handle`]
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

    /// Gracefully requests the capture thread to stop and waits for it to finish.
    ///
    /// This posts a WM_QUIT to the capture thread and joins it.
    ///
    /// # Errors
    ///
    /// - [`CaptureControlError::FailedToPostThreadMessage`] when posting WM_QUIT to the thread
    ///   fails and the thread is still running
    /// - [`CaptureControlError::FailedToJoinThread`] when joining the internal thread fails
    /// - [`CaptureControlError::ThreadHandleIsTaken`] when the thread handle was previously taken
    ///   via [`CaptureControl::into_thread_handle`]
    #[inline]
    pub fn stop(mut self) -> Result<(), CaptureControlError<E>> {
        self.halt_handle.store(true, atomic::Ordering::Relaxed);

        if let Some(thread_handle) = self.thread_handle.take() {
            let handle = thread_handle.as_raw_handle();
            let handle = HANDLE(handle);
            let thread_id = unsafe { GetThreadId(handle) };

            loop {
                match unsafe { PostThreadMessageW(thread_id, WM_QUIT, WPARAM::default(), LPARAM::default()) } {
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
/// Errors that can occur while initializing and running the Windows Graphics Capture pipeline.
pub enum GraphicsCaptureApiError<E> {
    /// Joining the worker thread failed (panic or OS-level join error).
    #[error("Failed to join thread")]
    FailedToJoinThread,
    /// Failed to initialize the Windows Runtime for multithreaded apartment.
    ///
    /// Occurs when `RoInitialize(RO_INIT_MULTITHREADED)` returns an error other than `S_FALSE`.
    #[error("Failed to initialize WinRT")]
    FailedToInitWinRT,
    /// Creating the dispatcher queue controller for the message loop failed.
    #[error("Failed to create dispatcher queue controller")]
    FailedToCreateDispatcherQueueController,
    /// Shutting down the dispatcher queue failed.
    #[error("Failed to shut down dispatcher queue")]
    FailedToShutdownDispatcherQueue,
    /// Registering the dispatcher queue completion handler failed.
    #[error("Failed to set dispatcher queue completed handler")]
    FailedToSetDispatcherQueueCompletedHandler,
    /// The provided item could not be converted into a `GraphicsCaptureItem`.
    ///
    /// This happens when
    /// [`crate::settings::TryIntoCaptureItemWithDetails::try_into_capture_item_with_details`]
    /// fails for the item passed in [`crate::settings::Settings`].
    #[error("Failed to convert item to `GraphicsCaptureItem`")]
    ItemConvertFailed,
    /// Underlying Direct3D (D3D11) error.
    ///
    /// Wraps [`crate::d3d11::Error`].
    #[error("DirectX error: {0}")]
    DirectXError(#[from] d3d11::Error),
    /// Error produced by the Windows Graphics Capture API wrapper.
    ///
    /// Wraps [`crate::graphics_capture_api::Error`].
    #[error("Graphics capture error: {0}")]
    GraphicsCaptureApiError(graphics_capture_api::Error),
    /// Error returned by the user handler when constructing it via
    /// [`GraphicsCaptureApiHandler::new`].
    #[error("New handler error: {0}")]
    NewHandlerError(E),
    /// Error returned by the user handler during frame processing via
    /// [`GraphicsCaptureApiHandler::on_frame_arrived`] or from
    /// [`GraphicsCaptureApiHandler::on_closed`].
    #[error("Frame handler error: {0}")]
    FrameHandlerError(E),
}

/// The context provided to the capture handler.
pub struct Context<Flags> {
    /// The flags that are retrieved from the settings.
    pub flags: Flags,
    /// The Direct3D device.
    pub device: ID3D11Device,
    /// The Direct3D device context.
    pub device_context: ID3D11DeviceContext,
}

/// Trait implemented by types that handle graphics capture events.
pub trait GraphicsCaptureApiHandler: Sized {
    /// The type of flags used to get the values from the settings.
    type Flags;

    /// The type of error that can occur during capture. The error will be returned from the
    /// [`CaptureControl`] and [`GraphicsCaptureApiHandler::start`] functions.
    type Error: Send + Sync;

    /// Starts the capture and takes control of the current thread.
    #[inline]
    fn start<T: TryIntoCaptureItemWithDetails>(
        settings: Settings<Self::Flags, T>,
    ) -> Result<(), GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send,
    {
        // Initialize WinRT
        static INIT_MTA: OnceLock<()> = OnceLock::new();
        INIT_MTA.get_or_init(|| {
            unsafe {
                CoIncrementMTAUsage().expect("Failed to increment MTA usage");
            };
        });

        match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
            Ok(_) => (),
            Err(e) => {
                if e.code() == S_FALSE {
                    // Already initialized
                } else {
                    return Err(GraphicsCaptureApiError::FailedToInitWinRT);
                }
            }
        }

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

        // Create Direct3D device and context
        let (d3d_device, d3d_device_context) = create_d3d_device()?;

        // Start capture
        let result = Arc::new(Mutex::new(None));

        let ctx =
            Context { flags: settings.flags, device: d3d_device.clone(), device_context: d3d_device_context.clone() };

        let callback = Arc::new(Mutex::new(Self::new(ctx).map_err(GraphicsCaptureApiError::NewHandlerError)?));

        let mut capture = GraphicsCaptureApi::new(
            d3d_device,
            d3d_device_context,
            settings
                .item
                .try_into_capture_item_with_details()
                .map_err(|_| GraphicsCaptureApiError::ItemConvertFailed)?,
            callback,
            settings.cursor_capture_settings,
            settings.draw_border_settings,
            settings.secondary_window_settings,
            settings.minimum_update_interval_settings,
            settings.dirty_region_settings,
            settings.color_format,
            thread_id,
            result.clone(),
        )
        .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;
        capture.start_capture().map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

        // Message loop
        let mut message = MSG::default();
        unsafe {
            while GetMessageW(&mut message, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        // Shut down dispatcher queue
        let async_action =
            controller.ShutdownQueueAsync().map_err(|_| GraphicsCaptureApiError::FailedToShutdownDispatcherQueue)?;

        async_action
            .SetCompleted(&AsyncActionCompletedHandler::new(move |_, _| -> WindowsResult<()> {
                unsafe { PostQuitMessage(0) };
                Ok(())
            }))
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
        // unsafe { RoUninitialize() }; // Not sure if this is needed here

        // Check handler result
        let result = result.lock().take();
        if let Some(e) = result {
            return Err(GraphicsCaptureApiError::FrameHandlerError(e));
        }

        Ok(())
    }

    /// Starts the capture without taking control of the current thread.
    #[inline]
    fn start_free_threaded<T: TryIntoCaptureItemWithDetails + Send + 'static>(
        settings: Settings<Self::Flags, T>,
    ) -> Result<CaptureControl<Self, Self::Error>, GraphicsCaptureApiError<Self::Error>>
    where
        Self: Send + 'static,
        <Self as GraphicsCaptureApiHandler>::Flags: Send,
    {
        let (halt_sender, halt_receiver) = mpsc::channel::<Arc<AtomicBool>>();
        let (callback_sender, callback_receiver) = mpsc::channel::<Arc<Mutex<Self>>>();

        let thread_handle = thread::spawn(move || -> Result<(), GraphicsCaptureApiError<Self::Error>> {
            // Initialize WinRT
            static INIT_MTA: OnceLock<()> = OnceLock::new();
            INIT_MTA.get_or_init(|| {
                unsafe {
                    CoIncrementMTAUsage().expect("Failed to increment MTA usage");
                };
            });

            match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
                Ok(_) => (),
                Err(e) => {
                    if e.code() == S_FALSE {
                        // Already initialized
                    } else {
                        return Err(GraphicsCaptureApiError::FailedToInitWinRT);
                    }
                }
            }

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

            // Create direct3d device and context
            let (d3d_device, d3d_device_context) = create_d3d_device()?;

            // Start capture
            let result = Arc::new(Mutex::new(None));

            let ctx = Context {
                flags: settings.flags,
                device: d3d_device.clone(),
                device_context: d3d_device_context.clone(),
            };

            let callback = Arc::new(Mutex::new(Self::new(ctx).map_err(GraphicsCaptureApiError::NewHandlerError)?));

            let mut capture = GraphicsCaptureApi::new(
                d3d_device,
                d3d_device_context,
                settings
                    .item
                    .try_into_capture_item_with_details()
                    .map_err(|_| GraphicsCaptureApiError::ItemConvertFailed)?,
                callback.clone(),
                settings.cursor_capture_settings,
                settings.draw_border_settings,
                settings.secondary_window_settings,
                settings.minimum_update_interval_settings,
                settings.dirty_region_settings,
                settings.color_format,
                thread_id,
                result.clone(),
            )
            .map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

            capture.start_capture().map_err(GraphicsCaptureApiError::GraphicsCaptureApiError)?;

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
                .SetCompleted(&AsyncActionCompletedHandler::new(move |_, _| -> Result<(), windows::core::Error> {
                    unsafe { PostQuitMessage(0) };
                    Ok(())
                }))
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
            // unsafe { RoUninitialize() }; // Not sure if this is needed here

            // Check handler result
            let result = result.lock().take();
            if let Some(e) = result {
                return Err(GraphicsCaptureApiError::FrameHandlerError(e));
            }

            Ok(())
        });

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

    /// Function that will be called to create the struct. The flags can be
    /// passed from settings.
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error>;

    /// Called every time a new frame is available.
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error>;

    /// Optional handler called when the capture item (usually a window) closes.
    #[inline]
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
