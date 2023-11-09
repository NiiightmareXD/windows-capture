use std::error::Error;

use log::warn;
use windows::{
    core::HSTRING,
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{
        Foundation::{BOOL, HWND, LPARAM, RECT, TRUE},
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

/// Used To Handle Window Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum WindowErrors {
    #[error("Failed To Get The Foreground Window")]
    NoActiveWindow,
    #[error("Failed To Find Window")]
    NotFound,
}

/// Represents A Windows
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Window {
    window: HWND,
}

impl Window {
    /// Get The Currently Active Foreground Window
    pub fn foreground() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let window = unsafe { GetForegroundWindow() };

        if window.0 == 0 {
            return Err(Box::new(WindowErrors::NoActiveWindow));
        }

        Ok(Self { window })
    }

    /// Create From A Window Name
    pub fn from_name(title: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let title = HSTRING::from(title);
        let window = unsafe { FindWindowW(None, &title) };

        if window.0 == 0 {
            return Err(Box::new(WindowErrors::NotFound));
        }

        Ok(Self { window })
    }

    /// Create From A Window Name Substring
    pub fn from_contains_name(title: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let windows = Self::enumerate()?;

        let mut target_window = None;
        for window in windows {
            if window.title()?.contains(title) {
                target_window = Some(window);
                break;
            }
        }

        Ok(target_window.map_or_else(|| Err(Box::new(WindowErrors::NotFound)), Ok)?)
    }

    /// Get Window Title
    pub fn title(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let len = unsafe { GetWindowTextLengthW(self.window) } + 1;

        let mut name = vec![0u16; len as usize];
        if len > 1 {
            let copied = unsafe { GetWindowTextW(self.window, &mut name) };
            if copied == 0 {
                return Ok(String::new());
            }
        }

        Ok(String::from_utf16_lossy(&name))
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

            if (ex_styles & WS_EX_TOOLWINDOW.0 as isize) != 0 {
                return false;
            }
            if (styles & WS_CHILD.0 as isize) != 0 {
                return false;
            }
        } else {
            warn!("GetClientRect Failed");
            return false;
        }

        true
    }

    /// Get A List Of All Windows
    pub fn enumerate() -> Result<Vec<Self>, Box<dyn Error + Send + Sync>> {
        let mut windows: Vec<Self> = Vec::new();

        unsafe {
            EnumChildWindows(
                GetDesktopWindow(),
                Some(Self::enum_windows_callback),
                LPARAM(std::ptr::addr_of_mut!(windows) as isize),
            )
            .ok()?;
        };

        Ok(windows)
    }

    /// Wait Until The Window Is The Currently Active Foreground Window
    pub fn activate(&self) {
        loop {
            if unsafe { GetForegroundWindow() } == self.window {
                break;
            }
        }
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
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(value: Window) -> Result<Self, Self::Error> {
        // Get Capture Item From HWND
        let window = value.as_raw_hwnd();

        let interop = windows::core::factory::<Self, IGraphicsCaptureItemInterop>()?;
        Ok(unsafe { interop.CreateForWindow(window)? })
    }
}
