use std::error::Error;

use windows::{
    core::ComInterface,
    Graphics::DirectX::Direct3D11::IDirect3DDevice,
    Win32::{
        Graphics::{
            Direct3D::{
                D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
                D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
                D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2, D3D_FEATURE_LEVEL_9_3,
            },
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
            },
            Dxgi::IDXGIDevice,
        },
        System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
    },
};

/// To Share DirectX Structs Between Threads
pub struct SendDirectX<T>(pub T);

impl<T> SendDirectX<T> {
    pub const fn new(device: T) -> Self {
        Self(device)
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for SendDirectX<T> {}

/// Used To Handle DirectX Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum DirectXErrors {
    #[error("Failed To Create DirectX Device With The Recommended Feature Level")]
    FeatureLevelNotSatisfied,
}

/// Create ID3D11Device And ID3D11DeviceContext
pub fn create_d3d_device()
-> Result<(ID3D11Device, ID3D11DeviceContext), Box<dyn Error + Send + Sync>> {
    // Set Feature Flags
    let feature_flags = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_9_3,
        D3D_FEATURE_LEVEL_9_2,
        D3D_FEATURE_LEVEL_9_1,
    ];

    // Try To Build A Hardware Device
    let mut d3d_device = None;
    let mut feature_level = D3D_FEATURE_LEVEL::default();
    let mut d3d_device_context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&feature_flags),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            Some(&mut feature_level),
            Some(&mut d3d_device_context),
        )?;
    };

    if feature_level != D3D_FEATURE_LEVEL_11_1 {
        return Err(Box::new(DirectXErrors::FeatureLevelNotSatisfied));
    }

    Ok((d3d_device.unwrap(), d3d_device_context.unwrap()))
}

/// Create A IDirect3DDevice From ID3D11Device
pub fn create_direct3d_device(
    d3d_device: &ID3D11Device,
) -> Result<IDirect3DDevice, Box<dyn Error + Send + Sync>> {
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    let device: IDirect3DDevice = inspectable.cast()?;

    Ok(device)
}
