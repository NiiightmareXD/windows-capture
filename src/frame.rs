use windows::{
    core::ComInterface,
    Graphics::DirectX::Direct3D11::IDirect3DSurface,
    Win32::{
        Graphics::Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    },
};

/// Pixels Color Representation
#[derive(Copy, Clone, Eq, PartialEq)]
#[repr(C)]
pub struct BGRA {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

/// Frame Struct Used To Crop And Get The Frame Buffer
pub struct Frame<'a> {
    surface: &'a IDirect3DSurface,
    d3d_device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
}

impl<'a> Frame<'a> {
    /// Craete A New Frame
    pub fn new(
        surface: &'a IDirect3DSurface,
        d3d_device: &'a ID3D11Device,
        context: &'a ID3D11DeviceContext,
    ) -> Self {
        Self {
            surface,
            d3d_device,
            context,
        }
    }

    /// Get The Frame Buffer
    pub fn buffer(&self) -> Result<FrameBuffer, Box<dyn std::error::Error>> {
        // Convert Surface To Texture
        let access = self.surface.cast::<IDirect3DDxgiInterfaceAccess>()?;
        let texture = unsafe { access.GetInterface::<ID3D11Texture2D>()? };

        // Texture Settings
        let mut texture_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut texture_desc) }
        texture_desc.Usage = D3D11_USAGE_STAGING;
        texture_desc.BindFlags = 0;
        texture_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        texture_desc.MiscFlags = 0;

        // Create A Temp Texture To Process On
        let mut texture_copy = None;
        unsafe {
            self.d3d_device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture_copy))?
        };
        let texture_copy = texture_copy.unwrap();

        // Copy The Real Texture To Temp Texture
        unsafe { self.context.CopyResource(&texture_copy, &texture) };

        // Map The Texture To Enable CPU Access
        let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context.Map(
                &texture_copy,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped_resource),
            )?
        };

        // Create A Slice From The Bits
        let slice: &[u8] = unsafe {
            std::slice::from_raw_parts(
                mapped_resource.pData as *const u8,
                (texture_desc.Height * mapped_resource.RowPitch) as usize,
            )
        };

        Ok(FrameBuffer {
            slice,
            width: texture_desc.Width,
            height: texture_desc.Height,
        })
    }

    /// Get The Raw IDirect3DSurface
    pub fn get_raw_surface(&self) -> &'a IDirect3DSurface {
        self.surface
    }
}

/// FrameBuffer Struct Used To Crop And Get The Buffer
pub struct FrameBuffer<'a> {
    slice: &'a [u8],
    width: u32,
    height: u32,
}

impl<'a> FrameBuffer<'a> {
    /// Create A New FrameBuffer
    pub const fn new(slice: &'a [u8], width: u32, height: u32) -> Self {
        Self {
            slice,
            width,
            height,
        }
    }

    /// Get The Cropped Version Of The Frame Buffer
    pub fn get_cropped(
        &self,
        start_width: u32,
        start_height: u32,
        width: u32,
        height: u32,
    ) -> Vec<BGRA> {
        let pixels = self.get();
        let mut cropped_pixels = Vec::with_capacity((width * height) as usize);

        for y in start_height..start_height + height {
            let row_start = y * self.width + start_width;
            let row_end = row_start + width;
            let row_slice = &pixels[row_start as usize..row_end as usize];
            cropped_pixels.extend_from_slice(row_slice);
        }

        cropped_pixels
    }

    /// Get The Frame Buffer
    pub const fn get(&self) -> &'a [BGRA] {
        let pixel_slice: &[BGRA] = unsafe {
            std::slice::from_raw_parts(
                self.slice.as_ptr() as *const BGRA,
                self.slice.len() / std::mem::size_of::<BGRA>(),
            )
        };

        pixel_slice
    }

    pub const fn get_width(&self) -> u32 {
        self.width
    }

    pub const fn get_height(&self) -> u32 {
        self.height
    }
}
