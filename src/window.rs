//! Utilities for querying and working with top-level windows.
//!
//! Provides [`crate::window::Window`] for finding and inspecting windows (title, process),
//! testing capture suitability, enumerating capturable windows, and converting
//! a window into a graphics capture item.
//!
//! Common tasks include:
//! - Getting the foreground window via [`crate::window::Window::foreground`].
//! - Finding by exact title via [`crate::window::Window::from_name`].
//! - Finding by substring via [`crate::window::Window::from_contains_name`].
//! - Enumerating capturable windows via [`crate::window::Window::enumerate`].
//! - Getting the owning process name via [`crate::window::Window::process_name`].
//! - Computing the title bar height via [`crate::window::Window::title_bar_height`].
//!
//! To acquire a [`crate::GraphicsCaptureItem`] for a window, use
//! `TryInto<GraphicsCaptureItemType>` for [`crate::window::Window`].
use std::ptr;

use crate::bindings::{
    DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute, EnumWindows, FindWindowW, GWL_EXSTYLE, GWL_STYLE,
    GetClientRect, GetDpiForWindow, GetForegroundWindow, GetModuleBaseNameW, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, GraphicsCaptureItem, HWND,
    IGraphicsCaptureItemInterop, IsWindowVisible, LPARAM, MONITOR_DEFAULTTONULL, MonitorFromWindow, OpenProcess,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_READ, RECT, WS_CHILD, WS_EX_TOOLWINDOW,
};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_core::{BOOL, HSTRING, PWSTR};

use crate::monitor::Monitor;
use crate::settings::GraphicsCaptureItemType;

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
/// Errors that can occur when querying or manipulating top-level windows via [`crate::window::Window`].
pub enum Error {
    /// There is no foreground window at the time of the call.
    ///
    /// Returned by [`crate::window::Window::foreground`].
    #[error("No active window found.")]
    NoActiveWindow,
    /// No window matched the provided title or substring.
    ///
    /// Returned by [`crate::window::Window::from_name`] and [`crate::window::Window::from_contains_name`].
    #[error("Failed to find a window with the name: {0}")]
    NotFound(String),
    /// Converting a UTF-16 Windows string to `String` failed.
    #[error("Failed to convert a Windows string from UTF-16")]
    FailedToConvertWindowsString,
    /// A Windows API call returned an error.
    ///
    /// Wraps [`windows_core::Error`].
    #[error("A Windows API call failed: {0}")]
    WindowsError(#[from] windows_core::Error),
}

/// Represents a window that can be captured.
///
/// # Example
/// ```no_run
/// use windows_capture::window::Window;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let window = Window::foreground()?;
///     println!("Foreground window title: {}", window.title()?);
///
///     Ok(())
/// }
/// ```
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Window {
    window: HWND,
}

unsafe impl Send for Window {}

impl Window {
    /// Returns the window that is currently in the foreground.
    ///
    /// # Errors
    ///
    /// - [`Error::NoActiveWindow`] when there is no foreground window
    #[inline]
    pub fn foreground() -> Result<Self, Error> {
        let window = unsafe { GetForegroundWindow() };

        if window.0.is_null() {
            return Err(Error::NoActiveWindow);
        }

        Ok(Self { window })
    }

    /// Finds a window by its exact title.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when the underlying `FindWindowW` call fails
    /// - [`Error::NotFound`] when no window with the specified title is found
    #[inline]
    pub fn from_name(title: &str) -> Result<Self, Error> {
        let hstring_title = HSTRING::from(title);
        let window = unsafe { FindWindowW(None, &hstring_title) };

        if window.0.is_null() {
            return Err(Error::NotFound(String::from(title)));
        }

        Ok(Self { window })
    }

    /// Finds a window whose title contains the given substring.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when enumerating windows fails
    /// - [`Error::FailedToConvertWindowsString`] when converting a window title from UTF-16 fails
    /// - [`Error::NotFound`] when no window title contains the specified substring
    #[inline]
    pub fn from_contains_name(title: &str) -> Result<Self, Error> {
        let windows = Self::enumerate()?;

        let mut target_window = None;
        for window in windows {
            if window.title()?.contains(title) {
                target_window = Some(window);
                break;
            }
        }

        target_window.map_or_else(|| Err(Error::NotFound(String::from(title))), Ok)
    }

    /// Returns the title of the window.
    ///
    /// # Errors
    ///
    /// - [`Error::FailedToConvertWindowsString`] when converting the window title from UTF-16 fails
    #[inline]
    pub fn title(&self) -> Result<String, Error> {
        let len = unsafe { GetWindowTextLengthW(self.window) };

        if len == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u16; usize::try_from(len).unwrap() + 1];
        let copied = unsafe { GetWindowTextW(self.window, PWSTR(buf.as_mut_ptr()), buf.len() as i32) };
        if copied == 0 {
            return Ok(String::new());
        }

        let name = String::from_utf16(&buf[..copied as usize]).map_err(|_| Error::FailedToConvertWindowsString)?;

        Ok(name)
    }

    /// Returns the process ID of the window.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when `GetWindowThreadProcessId` reports an error
    #[inline]
    pub fn process_id(&self) -> Result<u32, Error> {
        let mut id = 0;
        unsafe { GetWindowThreadProcessId(self.window, Some(&mut id)) };

        if id == 0 {
            return Err(Error::WindowsError(windows_core::Error::from_thread()));
        }

        Ok(id)
    }

    /// Returns the name of the process that owns the window.
    ///
    /// This function requires the `PROCESS_QUERY_INFORMATION` and `PROCESS_VM_READ` permissions.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when opening the process or querying its base module name fails
    /// - [`Error::FailedToConvertWindowsString`] when converting the process name from UTF-16 fails
    #[inline]
    pub fn process_name(&self) -> Result<String, Error> {
        let id = self.process_id()?;

        let process = unsafe { OpenProcess((PROCESS_QUERY_INFORMATION | PROCESS_VM_READ) as u32, false, id) };
        if process.0.is_null() {
            return Err(windows_core::Error::from_thread().into());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(process.0) };

        let mut name = vec![0u16; 260];
        let size = unsafe {
            GetModuleBaseNameW(
                crate::bindings::HANDLE(process.as_raw_handle()),
                None,
                PWSTR(name.as_mut_ptr()),
                name.len() as u32,
            )
        };

        if size == 0 {
            return Err(Error::WindowsError(windows_core::Error::from_thread()));
        }

        let name =
            String::from_utf16(&name.as_slice().iter().take_while(|ch| **ch != 0x0000).copied().collect::<Vec<u16>>())
                .map_err(|_| Error::FailedToConvertWindowsString)?;

        Ok(name)
    }

    /// Returns the monitor that has the largest area of intersection with the window.
    ///
    /// Returns `None` if the window does not intersect with any monitor.
    #[inline]
    #[must_use]
    pub fn monitor(&self) -> Option<Monitor> {
        let window = self.window;

        let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONULL as u32) };

        if monitor.0.is_null() { None } else { Some(Monitor::from_raw_hmonitor(monitor.0)) }
    }

    /// Returns the bounding rectangle of the window in screen coordinates.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when `GetWindowRect` fails
    #[inline]
    pub fn rect(&self) -> Result<RECT, Error> {
        let mut rect = RECT::default();
        let result = unsafe { GetWindowRect(self.window, &mut rect) };
        if result.as_bool() { Ok(rect) } else { Err(Error::WindowsError(windows_core::Error::from_thread())) }
    }

    /// Calculates the height of the window's title bar in pixels.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when `DwmGetWindowAttribute` or `GetClientRect` fails
    #[inline]
    pub fn title_bar_height(&self) -> Result<u32, Error> {
        let mut window_rect = RECT::default();
        let mut client_rect = RECT::default();

        unsafe {
            DwmGetWindowAttribute(
                self.window,
                DWMWA_EXTENDED_FRAME_BOUNDS as u32,
                &mut window_rect as *mut RECT as *mut std::ffi::c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .ok()
        }?;

        unsafe { GetClientRect(self.window, &mut client_rect).ok() }?;

        let window_height = window_rect.bottom - window_rect.top;
        let dpi = unsafe { GetDpiForWindow(self.window) };
        let client_height = (client_rect.bottom - client_rect.top) * dpi as i32 / 96;
        let actual_title_height = (window_height - client_height).max(0);

        Ok(actual_title_height as u32)
    }

    /// Checks whether the window is a valid target for capture.
    ///
    /// # Returns
    ///
    /// Returns `true` if the window is visible, not a tool window, and not a child window.
    /// Returns `false` otherwise.
    #[inline]
    #[must_use]
    pub fn is_valid(&self) -> bool {
        if !unsafe { IsWindowVisible(self.window).as_bool() } {
            return false;
        }

        let mut rect = RECT::default();
        let result = unsafe { GetClientRect(self.window, &mut rect).ok() };
        if result.is_ok() {
            #[cfg(target_pointer_width = "64")]
            let styles = unsafe { GetWindowLongPtrW(self.window, GWL_STYLE) };
            #[cfg(target_pointer_width = "64")]
            let ex_styles = unsafe { GetWindowLongPtrW(self.window, GWL_EXSTYLE) };

            #[cfg(target_pointer_width = "32")]
            let styles = unsafe { GetWindowLongPtrW(self.window, GWL_STYLE) as isize };
            #[cfg(target_pointer_width = "32")]
            let ex_styles = unsafe { GetWindowLongPtrW(self.window, GWL_EXSTYLE) as isize };

            if (ex_styles & isize::try_from(WS_EX_TOOLWINDOW).unwrap()) != 0 {
                return false;
            }
            if (styles & isize::try_from(WS_CHILD).unwrap()) != 0 {
                return false;
            }
        } else {
            return false;
        }

        true
    }

    /// Returns a list of all capturable windows.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when `EnumWindows` fails
    #[inline]
    pub fn enumerate() -> Result<Vec<Self>, Error> {
        let mut windows: Vec<Self> = Vec::new();

        unsafe {
            EnumWindows(Some(Self::enum_windows_callback), LPARAM(ptr::addr_of_mut!(windows) as isize)).ok()?;
        }

        Ok(windows)
    }

    /// Returns the width of the window in pixels.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when retrieving the window rectangle fails
    pub fn width(&self) -> Result<i32, Error> {
        let rect = self.rect()?;
        Ok(rect.right - rect.left)
    }

    /// Returns the height of the window in pixels.
    ///
    /// # Errors
    ///
    /// - [`Error::WindowsError`] when retrieving the window rectangle fails
    pub fn height(&self) -> Result<i32, Error> {
        let rect = self.rect()?;
        Ok(rect.bottom - rect.top)
    }

    /// Constructs a `crate::window::Window` instance from a raw `HWND` handle.
    #[inline]
    #[must_use]
    pub const fn from_raw_hwnd(hwnd: *mut std::ffi::c_void) -> Self {
        Self { window: HWND(hwnd) }
    }

    /// Returns the raw `HWND` handle of the window.
    #[inline]
    #[must_use]
    pub const fn as_raw_hwnd(&self) -> *mut std::ffi::c_void {
        self.window.0
    }

    // Callback used for enumerating all valid windows.
    #[inline]
    unsafe extern "system" fn enum_windows_callback(window: HWND, vec: LPARAM) -> BOOL {
        let windows = unsafe { &mut *(vec.0 as *mut Vec<Self>) };

        if Self::from_raw_hwnd(window.0).is_valid() {
            windows.push(Self { window });
        }

        BOOL(1)
    }
}

impl TryInto<GraphicsCaptureItemType> for Window {
    type Error = windows_core::Error;

    #[inline]
    fn try_into(self) -> Result<GraphicsCaptureItemType, Self::Error> {
        let window = HWND(self.as_raw_hwnd());

        let interop = windows_core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item = unsafe { interop.CreateForWindow(window)? };

        Ok(GraphicsCaptureItemType::Window((item, self)))
    }
}
