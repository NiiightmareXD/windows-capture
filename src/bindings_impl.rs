//! Threading traits previously supplied by the umbrella bindings.

use crate::bindings::*;

// Direct3D/DXGI interface references can be cloned, queried, and released across
// threads. Their operational methods are unsafe: callers must still serialize
// immediate-context and DXGI access. This preserves the windows 0.62 contract.
// https://learn.microsoft.com/windows/win32/direct3d11/overviews-direct3d-11-render-multi-thread-intro
macro_rules! free_threaded_reference {
    ($($interface:ty),+ $(,)?) => {$(
        unsafe impl Send for $interface {}
        unsafe impl Sync for $interface {}
    )+};
}

free_threaded_reference!(
    ID3D11Device,
    ID3D11DeviceContext,
    ID3D11Texture2D,
    ID3D11RenderTargetView,
    IDXGIDevice,
    IDXGIDevice4,
    IDXGIOutput6,
    IDXGIOutputDuplication,
    IDXGISurface,
);
