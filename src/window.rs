use log::warn;
use thiserror::Error;
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

/// Used To Handle Internal Errors
#[derive(Error, Debug)]
pub enum WindowErrors {
    #[error("Failed To Find Window")]
    NotFound,
    #[error("Unknown Error")]
    Unknown,
}

/// Represents A Windows
pub struct Window {
    window: HWND,
}

impl Window {
    /// Get The Currently Active Foreground Window
    pub fn get_foreground() -> Self {
        let window = unsafe { GetForegroundWindow() };
        Self { window }
    }

    /// Crate From A HWND
    pub const fn from_hwnd(window: HWND) -> Self {
        Self { window }
    }

    /// Create From A Window Name
    pub fn from_window_name(title: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let title = HSTRING::from(title);
        let window = unsafe { FindWindowW(None, &title) };

        if window.0 == 0 {
            return Err(Box::new(WindowErrors::NotFound));
        }

        Ok(Self { window })
    }

    /// Get Window Title
    pub fn get_window_title(window: HWND) -> Result<String, Box<dyn std::error::Error>> {
        let len = unsafe { GetWindowTextLengthW(window) } + 1;

        let mut name = vec![0u16; len as usize];
        if len > 1 {
            let copied = unsafe { GetWindowTextW(window, &mut name) };
            if copied == 0 {
                return Ok(String::new());
            }
        }

        Ok(String::from_utf16_lossy(&name))
    }

    /// Check If The Window Is A Valid Window
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
        }

        true
    }

    /// Get A List Of All Windows
    pub fn get_windows() -> Result<Vec<HWND>, Box<dyn std::error::Error>> {
        let mut windows: Vec<HWND> = Vec::new();

        unsafe {
            EnumChildWindows(
                GetDesktopWindow(),
                Some(Self::enum_windows_callback),
                LPARAM(&mut windows as *mut Vec<HWND> as isize),
            )
            .ok()?
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

    /// Get The Raw HWND
    pub const fn get_raw_hwnd(&self) -> HWND {
        self.window
    }

    unsafe extern "system" fn enum_windows_callback(window: HWND, vec: LPARAM) -> BOOL {
        let windows = &mut *(vec.0 as *mut Vec<HWND>);

        if Self::is_window_valid(window) {
            windows.push(window);
        }

        TRUE
    }
}

// Automatically Convert Window To GraphicsCaptureItem
impl From<Window> for GraphicsCaptureItem {
    fn from(value: Window) -> Self {
        // Get Capture Item From HMONITOR
        let window = value.get_raw_hwnd();

        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
        unsafe { interop.CreateForWindow(window).unwrap() }
    }
}
