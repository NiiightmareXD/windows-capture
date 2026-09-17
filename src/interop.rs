//! Windows types used by the public capture and encoding APIs.
//!
//! These types share COM identity through [`windows_core::Interface`]. Use
//! `Interface::cast` to exchange interfaces with other Windows bindings.

pub use crate::bindings::{
    D3D11_MAP_READ_WRITE, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_OUTDUPL_DESC, DXGI_OUTDUPL_FRAME_INFO, DataReader,
    Direct3D11CaptureFrame, GraphicsCaptureItem, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, IDXGIDevice4,
    IDXGIOutput6, IDXGIOutputDuplication, IDirect3DDevice, IDirect3DSurface, IRandomAccessStream,
    InMemoryRandomAccessStream, RECT,
};
pub use windows_time::TimeSpan;
