use std::error::Error;

use windows::{
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{
        Foundation::{BOOL, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::{
            EnumDisplayMonitors, MonitorFromPoint, HDC, HMONITOR, MONITOR_DEFAULTTOPRIMARY,
        },
        System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
    },
};

/// Used To Handle Monitor Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum MonitorErrors {
    #[error("Failed To Find Monitor")]
    NotFound,
}

/// Represents A Monitor Device
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Monitor {
    monitor: HMONITOR,
}

impl Monitor {
    /// Get The Primary Monitor
    #[must_use]
    pub fn primary() -> Self {
        let point = POINT { x: 0, y: 0 };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };

        Self { monitor }
    }

    /// Get The Monitor From It's Index
    pub fn from_index(index: usize) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let monitor = Self::enumerate()?;
        let monitor = match monitor.get(index) {
            Some(monitor) => *monitor,
            None => return Err(Box::new(MonitorErrors::NotFound)),
        };

        Ok(Self { monitor })
    }

    /// Get A List Of All Monitors
    pub fn enumerate() -> Result<Vec<HMONITOR>, Box<dyn Error + Send + Sync>> {
        let mut monitors: Vec<HMONITOR> = Vec::new();

        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(Self::enum_monitors_callback),
                LPARAM(std::ptr::addr_of_mut!(monitors) as isize),
            )
            .ok()?;
        };

        Ok(monitors)
    }

    /// Create From A Raw HMONITOR
    #[must_use]
    pub const fn from_raw_hmonitor(monitor: HMONITOR) -> Self {
        Self { monitor }
    }

    /// Get The Raw HMONITOR
    #[must_use]
    pub const fn as_raw_hmonitor(&self) -> HMONITOR {
        self.monitor
    }

    // Callback Used For Enumerating All Monitors
    unsafe extern "system" fn enum_monitors_callback(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        vec: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(vec.0 as *mut Vec<HMONITOR>);

        monitors.push(monitor);

        TRUE
    }
}

// Automatically Convert Monitor To GraphicsCaptureItem
impl TryFrom<Monitor> for GraphicsCaptureItem {
    type Error = Box<dyn Error + Send + Sync>;

    fn try_from(value: Monitor) -> Result<Self, Self::Error> {
        // Get Capture Item From HMONITOR
        let monitor = value.as_raw_hmonitor();

        let interop = windows::core::factory::<Self, IGraphicsCaptureItemInterop>()?;
        Ok(unsafe { interop.CreateForMonitor(monitor)? })
    }
}
