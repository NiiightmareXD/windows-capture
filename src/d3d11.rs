use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2, D3D_FEATURE_LEVEL_9_3,
    D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::core::Interface;

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
/// Errors that can occur when creating or working with Direct3D devices and textures.
pub enum Error {
    /// The created device does not support at least feature level 11.0.
    #[error("Failed to create DirectX device with the recommended feature levels")]
    FeatureLevelNotSatisfied,
    /// A Windows Runtime/Win32 API call failed.
    ///
    /// Wraps [`windows::core::Error`].
    #[error("Windows API Error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

/// A wrapper to send a DirectX device across threads.
pub struct SendDirectX<T>(pub T);

impl<T> SendDirectX<T> {
    /// Constructs a new `SendDirectX` instance.
    #[inline]
    #[must_use]
    pub const fn new(device: T) -> Self {
        Self(device)
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for SendDirectX<T> {}

/// Creates an [`windows::Win32::Graphics::Direct3D11::ID3D11Device`] and an
/// [`windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext`].
///
/// # Errors
///
/// - [`Error::WindowsError`] when the underlying `D3D11CreateDevice` call fails
/// - [`Error::FeatureLevelNotSatisfied`] when the created device does not support at least feature
///   level 11.0
#[inline]
pub fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext), Error> {
    // Array of Direct3D feature levels.
    // The feature levels are listed in descending order of capability.
    // The highest feature level supported by the system is at index 0.
    // The lowest feature level supported by the system is at the last index.
    let feature_flags = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_9_3,
        D3D_FEATURE_LEVEL_9_2,
        D3D_FEATURE_LEVEL_9_1,
    ];

    let mut d3d_device = None;
    let mut feature_level = D3D_FEATURE_LEVEL::default();
    let mut d3d_device_context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_flags),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            Some(&mut feature_level),
            Some(&mut d3d_device_context),
        )?;
    };

    if feature_level.0 < D3D_FEATURE_LEVEL_11_0.0 {
        return Err(Error::FeatureLevelNotSatisfied);
    }

    Ok((d3d_device.unwrap(), d3d_device_context.unwrap()))
}

/// Creates an [`windows::Graphics::DirectX::Direct3D11::IDirect3DDevice`] from an
/// [`windows::Win32::Graphics::Direct3D11::ID3D11Device`].
///
/// # Errors
///
/// - [`Error::WindowsError`] when creating the Direct3D11 device wrapper fails
#[inline]
pub fn create_direct3d_device(d3d_device: &ID3D11Device) -> Result<IDirect3DDevice, Error> {
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    let device: IDirect3DDevice = inspectable.cast()?;

    Ok(device)
}

/// Reusable CPU-read/write staging texture wrapper.
pub struct StagingTexture {
    inner: ID3D11Texture2D,
    desc: D3D11_TEXTURE2D_DESC,
    is_mapped: bool,
}

impl StagingTexture {
    /// Create a staging texture suitable for CPU read/write with the given geometry/format.
    pub fn new(device: &ID3D11Device, width: u32, height: u32, format: DXGI_FORMAT) -> Result<Self, Error> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: (D3D11_CPU_ACCESS_READ.0 | D3D11_CPU_ACCESS_WRITE.0) as u32,
            MiscFlags: 0,
        };

        let mut tex = None;
        unsafe {
            device.CreateTexture2D(&desc, None, Some(&mut tex))?;
        }
        Ok(Self { inner: tex.unwrap(), desc, is_mapped: false })
    }

    /// Gets the underlying [`windows::Win32::Graphics::Direct3D11::ID3D11Texture2D`].
    #[inline]
    #[must_use]
    pub const fn texture(&self) -> &ID3D11Texture2D {
        &self.inner
    }

    /// Gets the description of the texture.
    #[inline]
    #[must_use]
    pub const fn desc(&self) -> D3D11_TEXTURE2D_DESC {
        self.desc
    }

    /// Checks if the texture is currently mapped.
    #[inline]
    #[must_use]
    pub const fn is_mapped(&self) -> bool {
        self.is_mapped
    }

    /// Marks the texture as mapped or unmapped.
    #[inline]
    pub const fn set_mapped(&mut self, mapped: bool) {
        self.is_mapped = mapped;
    }

    /// Validate an externally constructed texture as a CPU staging texture.
    /// The texture must have been created with `D3D11_USAGE_STAGING` usage and
    /// `D3D11_CPU_ACCESS_READ` and `D3D11_CPU_ACCESS_WRITE` CPU access flags.
    pub fn from_raw_checked(tex: ID3D11Texture2D) -> Option<Self> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let is_staging = desc.Usage == D3D11_USAGE_STAGING;
        let has_cpu_rw = (desc.CPUAccessFlags & (D3D11_CPU_ACCESS_READ.0 | D3D11_CPU_ACCESS_WRITE.0) as u32) != 0;

        if !is_staging || !has_cpu_rw {
            return None;
        }

        Some(Self { inner: tex, desc, is_mapped: false })
    }
}
