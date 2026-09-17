use std::sync::Arc;

use windows_core::Interface;

use crate::bindings::*;
use crate::d3d11::{MappedStagingTexture, StagingTexture};
use crate::encoder::{ImageEncoder, ImageEncoderPixelFormat, ImageFormat};
use crate::winrt::WinRT;

#[test]
fn warp_texture_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    // WARP exercises the real D3D11 ABI without requiring a GPU or display in CI.
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT as u32,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION as u32,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .ok()?;
    }
    let device = device.unwrap();
    let context = context.unwrap();
    let mut texture = StagingTexture::new(&device, 2, 2, DXGI_FORMAT_B8G8R8A8_UNORM)?;
    {
        let mut mapped = MappedStagingTexture::map_borrowed(&context, &mut texture)?;
        mapped.as_mut_slice(2).fill(0x7b);
    }
    assert!(!texture.is_mapped());
    {
        let mapped = MappedStagingTexture::map_borrowed(&context, &mut texture)?;
        let pitch = mapped.row_pitch() as usize;
        assert_eq!(&mapped.as_slice(2)[..8], &[0x7b; 8]);
        assert_eq!(&mapped.as_slice(2)[pitch..pitch + 8], &[0x7b; 8]);
    }
    let dxgi: IDXGIDevice = device.cast()?;
    let device_again: ID3D11Device = dxgi.cast()?;
    assert_eq!(device, device_again);
    assert!(StagingTexture::new(&device, 0, 0, DXGI_FORMAT_B8G8R8A8_UNORM).is_err());
    Ok(())
}

#[test]
fn winrt_image_encoding_and_event_lifetime() -> Result<(), Box<dyn std::error::Error>> {
    let _winrt = WinRT::new()?;
    let encoder = ImageEncoder::new(ImageFormat::Png, ImageEncoderPixelFormat::Rgba8)?;
    let png = encoder.encode(&[255, 0, 0, 255], 1, 1)?;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");

    let video = VideoStreamDescriptor::Create(&VideoEncodingProperties::CreateUncompressed(
        &MediaEncodingSubtypes::Bgra8()?,
        2,
        2,
    )?)?;
    let audio = AudioStreamDescriptor::Create(&AudioEncodingProperties::CreatePcm(48_000, 2, 16)?)?;
    let source = MediaStreamSource::CreateFromDescriptors(&video, &audio)?;
    let captured = Arc::new(());
    let weak = Arc::downgrade(&captured);
    let registration = crate::events::starting(&source, move |_, _| {
        let _ = &captured;
        Ok(())
    })?;
    assert!(weak.upgrade().is_some());
    drop(registration);
    assert!(weak.upgrade().is_none(), "dropping the registration must release its callback");

    let handler =
        TypedEventHandler::<MediaStreamSource, MediaStreamSourceStartingEventArgs>::new(
            |_, _| Err(E_INVALIDARG.into()),
        );
    assert_eq!(handler.Invoke(&source, None).unwrap_err().code(), E_INVALIDARG);
    Ok(())
}

#[test]
fn invalid_window_reports_win32_failure() {
    let window = crate::window::Window::from_raw_hwnd(std::ptr::null_mut());
    assert!(window.rect().is_err());
    assert!(window.process_id().is_err());
    assert!(window.monitor().is_none());
}
