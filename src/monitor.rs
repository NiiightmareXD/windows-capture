use std::{error::Error, mem, ptr};

use windows::{
    core::PCWSTR,
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{
        Devices::Display::{
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
            DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
            DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
            DISPLAYCONFIG_TARGET_DEVICE_NAME_FLAGS, DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY,
            QDC_ONLY_ACTIVE_PATHS,
        },
        Foundation::{BOOL, LPARAM, POINT, RECT, TRUE},
        Graphics::Gdi::{
            EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, MonitorFromPoint,
            DISPLAY_DEVICEW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTOPRIMARY,
        },
        System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop,
    },
};

/// Used To Handle Monitor Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum MonitorErrors {
    #[error("Failed To Find Monitor")]
    NotFound,
    #[error("Failed To Find Monitor Name")]
    NameNotFound,
    #[error("Monitor Index Is Lower Than One")]
    IndexIsLowerThanOne,
    #[error("Failed To Get Monitor Info")]
    FailedToGetMonitorInfo,
    #[error("Failed To Get Monitor Name")]
    FailedToGetMonitorName,
}

/// Represents A Monitor Device
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Monitor {
    monitor: HMONITOR,
}

impl Monitor {
    /// Get The Primary Monitor
    pub fn primary() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let point = POINT { x: 0, y: 0 };
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };

        if monitor.is_invalid() {
            return Err(Box::new(MonitorErrors::NotFound));
        }

        Ok(Self { monitor })
    }

    /// Get The Monitor From It's Index
    pub fn from_index(index: usize) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if index < 1 {
            return Err(Box::new(MonitorErrors::IndexIsLowerThanOne));
        }

        let monitor = Self::enumerate()?;
        let monitor = match monitor.get(index - 1) {
            Some(monitor) => *monitor,
            None => return Err(Box::new(MonitorErrors::NotFound)),
        };

        Ok(monitor)
    }

    pub fn index(&self) -> Result<usize, Box<dyn Error + Send + Sync>> {
        let device_name = self.device_name()?;
        Ok(device_name.replace("\\\\.\\DISPLAY1", "to").parse()?)
    }

    /// Get Monitor Name
    pub fn name(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut monitor_info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: mem::size_of::<MONITORINFOEXW>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            },
            szDevice: [0; 32],
        };
        if unsafe {
            !GetMonitorInfoW(
                self.as_raw_hmonitor(),
                std::ptr::addr_of_mut!(monitor_info).cast(),
            )
            .as_bool()
        } {
            return Err(Box::new(MonitorErrors::FailedToGetMonitorInfo));
        }

        let mut number_of_paths = 0;
        let mut number_of_modes = 0;
        unsafe {
            GetDisplayConfigBufferSizes(
                QDC_ONLY_ACTIVE_PATHS,
                &mut number_of_paths,
                &mut number_of_modes,
            )?;
        };

        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); number_of_paths as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); number_of_modes as usize];
        unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut number_of_paths,
                paths.as_mut_ptr(),
                &mut number_of_modes,
                modes.as_mut_ptr(),
                None,
            )
        }?;

        for path in paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                viewGdiDeviceName: [0; 32],
            };

            let device_name = self.device_name()?;
            let view_gdi_device_name = String::from_utf16(
                &monitor_info
                    .szDevice
                    .as_slice()
                    .iter()
                    .take_while(|ch| **ch != 0x0000)
                    .copied()
                    .collect::<Vec<_>>(),
            )?;

            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } == 0
                && device_name == view_gdi_device_name
            {
                let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
                    header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                        size: mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                        adapterId: path.sourceInfo.adapterId,
                        id: path.targetInfo.id,
                    },
                    flags: DISPLAYCONFIG_TARGET_DEVICE_NAME_FLAGS::default(),
                    outputTechnology: DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY::default(),
                    edidManufactureId: 0,
                    edidProductCodeId: 0,
                    connectorInstance: 0,
                    monitorFriendlyDeviceName: [0; 64],
                    monitorDevicePath: [0; 128],
                };

                if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } == 0 {
                    let name = String::from_utf16(
                        &target
                            .monitorFriendlyDeviceName
                            .as_slice()
                            .iter()
                            .take_while(|ch| **ch != 0x0000)
                            .copied()
                            .collect::<Vec<_>>(),
                    )?;
                    return Ok(name);
                } else {
                    return Err(Box::new(MonitorErrors::FailedToGetMonitorInfo));
                }
            }
        }

        Err(Box::new(MonitorErrors::NameNotFound))
    }

    /// Get Monitor Device Name
    pub fn device_name(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut monitor_info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: mem::size_of::<MONITORINFOEXW>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            },
            szDevice: [0; 32],
        };
        if unsafe {
            !GetMonitorInfoW(
                self.as_raw_hmonitor(),
                std::ptr::addr_of_mut!(monitor_info).cast(),
            )
            .as_bool()
        } {
            return Err(Box::new(MonitorErrors::FailedToGetMonitorInfo));
        }

        let device_name = String::from_utf16(
            &monitor_info
                .szDevice
                .as_slice()
                .iter()
                .take_while(|ch| **ch != 0x0000)
                .copied()
                .collect::<Vec<_>>(),
        )?;

        Ok(device_name)
    }

    /// Get Monitor Device String
    pub fn device_string(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut monitor_info = MONITORINFOEXW {
            monitorInfo: MONITORINFO {
                cbSize: mem::size_of::<MONITORINFOEXW>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            },
            szDevice: [0; 32],
        };
        if unsafe {
            !GetMonitorInfoW(
                self.as_raw_hmonitor(),
                std::ptr::addr_of_mut!(monitor_info).cast(),
            )
            .as_bool()
        } {
            return Err(Box::new(MonitorErrors::FailedToGetMonitorInfo));
        }

        let mut display_device = DISPLAY_DEVICEW {
            cb: mem::size_of::<DISPLAY_DEVICEW>() as u32,
            DeviceName: [0; 32],
            DeviceString: [0; 128],
            StateFlags: 0,
            DeviceID: [0; 128],
            DeviceKey: [0; 128],
        };

        if unsafe {
            !EnumDisplayDevicesW(
                PCWSTR::from_raw(monitor_info.szDevice.as_mut_ptr()),
                0,
                &mut display_device,
                0,
            )
            .as_bool()
        } {
            return Err(Box::new(MonitorErrors::FailedToGetMonitorName));
        }

        let device_string = String::from_utf16(
            &display_device
                .DeviceString
                .as_slice()
                .iter()
                .take_while(|ch| **ch != 0x0000)
                .copied()
                .collect::<Vec<_>>(),
        )?;

        Ok(device_string)
    }

    /// Get A List Of All Monitors
    pub fn enumerate() -> Result<Vec<Self>, Box<dyn Error + Send + Sync>> {
        let mut monitors: Vec<Self> = Vec::new();

        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(Self::enum_monitors_callback),
                LPARAM(ptr::addr_of_mut!(monitors) as isize),
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
        let monitors = &mut *(vec.0 as *mut Vec<Self>);

        monitors.push(Self { monitor });

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
