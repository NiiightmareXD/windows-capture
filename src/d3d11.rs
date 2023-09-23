use log::warn;
use windows::{
    core::ComInterface,
    Graphics::DirectX::Direct3D11::IDirect3DDevice,
    Win32::{
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_SDK_VERSION,
            },
            Dxgi::{IDXGIDevice, DXGI_ERROR_UNSUPPORTED},
        },
        System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice,
    },
};

pub struct SendDirectX<T> {
    pub inner: T,
}

impl<T> SendDirectX<T> {
    pub fn new(device: T) -> Self {
        Self { inner: device }
    }
}

unsafe impl<T> Send for SendDirectX<T> {}

pub fn create_d3d_device() -> Result<ID3D11Device, Box<dyn std::error::Error>> {
    // Try To Build A Hardware Device
    let mut d3d_device = None;
    let result = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            None,
        )
    };

    // If Failed Switch To Warp
    if result.as_ref().is_err() {
        if result.as_ref().err().unwrap() == &DXGI_ERROR_UNSUPPORTED.into() {
            warn!("Failed To Create D3D_DRIVER_TYPE_HARDWARE DirectX 11 Device");
            unsafe {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    None,
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    None,
                    D3D11_SDK_VERSION,
                    Some(&mut d3d_device),
                    None,
                    None,
                )?;
            };
        } else {
            result?;
        }
    }

    Ok(d3d_device.unwrap())
}

pub fn create_direct3d_device(
    d3d_device: &ID3D11Device,
) -> Result<IDirect3DDevice, Box<dyn std::error::Error>> {
    // Create A IDirect3DDevice From ID3D11Device
    let dxgi_device: IDXGIDevice = d3d_device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)? };
    let device: IDirect3DDevice = inspectable.cast()?;

    Ok(device)
}
