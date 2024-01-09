use std::{ptr, string::FromUtf16Error};

use windows::{
    core::HSTRING,
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{
        Foundation::{BOOL, HWND, LPARAM, RECT, TRUE},
        Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONULL},
        System::{
            Threading::GetCurrentProcessId, WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
        },
        UI::WindowsAndMessaging::{
            EnumChildWindows, FindWindowW, GetClientRect, GetDesktopWindow, GetForegroundWindow,
            GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, WS_CHILD, WS_EX_TOOLWINDOW,
        },
    },
};

use crate::monitor::Monitor;

/// Used To Handle Window Errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed To Get The Foreground Window")]
    NoActiveWindow,
    #[error("Failed To Find Window")]
    NotFound,
    #[error(transparent)]
    FailedToConvertWindowsString(#[from] FromUtf16Error),
    #[error(transparent)]
    WindowsError(#[from] windows::core::Error),
}

/// Represents A Windows
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Window {
    window: HWND,
}

impl Window {
    /// Get The Currently Active Window
    pub fn foreground() -> Result<Self, Error> {
        let window = unsafe { GetForegroundWindow() };

        if window.0 == 0 {
            return Err(Error::NoActiveWindow);
        }

        Ok(Self { window })
    }

    /// Create From A Window Name
    pub fn from_name(title: &str) -> Result<Self, Error> {
        let title = HSTRING::from(title);
        let window = unsafe { FindWindowW(None, &title) };

        if window.0 == 0 {
            return Err(Error::NotFound);
        }

        Ok(Self { window })
    }

    /// Create From A Window Name Substring
    pub fn from_contains_name(title: &str) -> Result<Self, Error> {
        let windows = Self::enumerate()?;

        let mut target_window = None;
        for window in windows {
            if window.title()?.contains(title) {
                target_window = Some(window);
                break;
            }
        }

        target_window.map_or_else(|| Err(Error::NotFound), Ok)
    }

    /// Get Window Title
    pub fn title(&self) -> Result<String, Error> {
        let len = unsafe { GetWindowTextLengthW(self.window) };

        let mut name = vec![0u16; usize::try_from(len).unwrap() + 1];
        if len >= 1 {
            let copied = unsafe { GetWindowTextW(self.window, &mut name) };
            if copied == 0 {
                return Ok(String::new());
            }
        }

        let name = String::from_utf16(
            &name
                .as_slice()
                .iter()
                .take_while(|ch| **ch != 0x0000)
                .copied()
                .collect::<Vec<_>>(),
        )?;

        Ok(name)
    }

    /// Get The Monitor That Has The Largest Area Of Intersection With The Window, None Means Window Doesn't Intersect With Any Monitor
    #[must_use]
    pub fn monitor(&self) -> Option<Monitor> {
        let window = self.window;

        let monitor = unsafe { MonitorFromWindow(window, MONITOR_DEFAULTTONULL) };

        if monitor.is_invalid() {
            None
        } else {
            Some(Monitor::from_raw_hmonitor(monitor))
        }
    }

    /// Check If The Window Is A Valid Window
    #[must_use]
    pub fn is_window_valid(window: HWND) -> bool {
        if !unsafe { IsWindowVisible(window).as_bool() } {
            return false;
        }

        let mut id = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut id)) };
        if id == unsafe { GetCurrentProcessId() } {
            return false;
        }

        let mut rect = RECT::default();
        let result = unsafe { GetClientRect(window, &mut rect) };
        if result.is_ok() {
            let styles = unsafe { GetWindowLongPtrW(window, GWL_STYLE) };
            let ex_styles = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) };

            if (ex_styles & isize::try_from(WS_EX_TOOLWINDOW.0).unwrap()) != 0 {
                return false;
            }
            if (styles & isize::try_from(WS_CHILD.0).unwrap()) != 0 {
                return false;
            }
        } else {
            return false;
        }

        true
    }

    /// Get A List Of All Windows
    pub fn enumerate() -> Result<Vec<Self>, Error> {
        let mut windows: Vec<Self> = Vec::new();

        unsafe {
            EnumChildWindows(
                GetDesktopWindow(),
                Some(Self::enum_windows_callback),
                LPARAM(ptr::addr_of_mut!(windows) as isize),
            )
            .ok()?;
        };

        Ok(windows)
    }

    /// Create From A Raw HWND
    #[must_use]
    pub const fn from_raw_hwnd(window: HWND) -> Self {
        Self { window }
    }

    /// Get The Raw HWND
    #[must_use]
    pub const fn as_raw_hwnd(&self) -> HWND {
        self.window
    }

    // Callback Used For Enumerating All Windows
    unsafe extern "system" fn enum_windows_callback(window: HWND, vec: LPARAM) -> BOOL {
        let windows = &mut *(vec.0 as *mut Vec<Self>);

        if Self::is_window_valid(window) {
            windows.push(Self { window });
        }

        TRUE
    }
}

// Automatically Convert Window To GraphicsCaptureItem
impl TryFrom<Window> for GraphicsCaptureItem {
    type Error = Error;

    fn try_from(value: Window) -> Result<Self, Self::Error> {
        // Get Capture Item From HWND
        let window = value.as_raw_hwnd();

        let interop = windows::core::factory::<Self, IGraphicsCaptureItemInterop>()?;
        Ok(unsafe { interop.CreateForWindow(window)? })
    }
}
