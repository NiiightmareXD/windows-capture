use crate::bindings::{
    CO_MTA_USAGE_COOKIE, CoDecrementMTAUsage, CoIncrementMTAUsage, RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
    S_FALSE,
};

/// Panic safe wrapper around `CoIncrementMTAUsage`.
struct WinMTACookie {
    cookie: CO_MTA_USAGE_COOKIE,
}

impl WinMTACookie {
    /// Increments the current threads MTA usage.
    pub fn new() -> windows_core::Result<Self> {
        Ok(Self { cookie: unsafe { CoIncrementMTAUsage()? } })
    }
}

impl Drop for WinMTACookie {
    fn drop(&mut self) {
        let _ = unsafe { CoDecrementMTAUsage(self.cookie) };
    }
}

/// Panic safe wrapper for WinRT api initialization.
pub struct WinRT {
    _cookie: WinMTACookie,
}

impl WinRT {
    /// Initializes WinRT apis on the current thread.
    pub fn new() -> windows_core::Result<Self> {
        let cookie = WinMTACookie::new()?;

        if let Err(e) = unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok() }
            && e.code() != S_FALSE
        {
            return Err(e);
        }

        Ok(Self { _cookie: cookie })
    }
}

impl Drop for WinRT {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}
