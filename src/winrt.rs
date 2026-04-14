use windows::Win32::Foundation::S_FALSE;
use windows::Win32::System::Com::{CO_MTA_USAGE_COOKIE, CoDecrementMTAUsage, CoIncrementMTAUsage};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

/// Panic safe wrapper around `CoIncrementMTAUsage`.
struct WinMTACookie {
    cookie: CO_MTA_USAGE_COOKIE,
}

impl WinMTACookie {
    /// Increments the current threads MTA usage.
    pub fn new() -> Self {
        Self { cookie: unsafe { CoIncrementMTAUsage() }.expect("Failed to increment MTA usage") }
    }
}

impl Drop for WinMTACookie {
    fn drop(&mut self) {
        let _ = unsafe { CoDecrementMTAUsage(self.cookie) };
    }
}

/// Panic safe wrapper for WinRT api initialization.
pub struct WinRT {
    cookie: WinMTACookie,
}

impl WinRT {
    /// Initializes WinRT apis on the current thread.
    pub fn new() -> Self {
        let cookie = WinMTACookie::new();

        if let Err(e) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            && e.code() != S_FALSE
        {
            panic!("Failed to initialize WinRT");
        }

        Self { cookie }
    }
}

impl Drop for WinRT {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };

        let _ = self.cookie;
    }
}
