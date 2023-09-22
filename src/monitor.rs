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

/// Represents A Monitor Device
pub struct Monitor {
    monitor: HMONITOR,
}

impl Monitor {
    /// Get The Primary Monitor
    pub fn get_primary() -> Self {
        let point = POINT { x: 0, y: 0 };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };

        Self { monitor }
    }

    /// Create From A HMONITOR
    pub fn from_hmonitor(monitor: HMONITOR) -> Self {
        Self { monitor }
    }

    /// Get A List Of All Monitors
    pub fn list_monitors() -> Result<Vec<HMONITOR>, Box<dyn std::error::Error>> {
        let mut monitors: Vec<HMONITOR> = Vec::new();

        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(Self::enum_monitors_callback),
                LPARAM(&mut monitors as *mut Vec<HMONITOR> as isize),
            )
            .ok()?
        };

        Ok(monitors)
    }

    /// Get The Raw HMONITOR
    pub fn get_raw_hmonitor(&self) -> HMONITOR {
        self.monitor
    }

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
impl From<Monitor> for GraphicsCaptureItem {
    fn from(value: Monitor) -> Self {
        // Get Capture Item From HMONITOR
        let monitor = value.get_raw_hmonitor();

        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().unwrap();
        unsafe { interop.CreateForMonitor(monitor).unwrap() }
    }
}
