use windows::{
    core::ComInterface,
    Graphics::DirectX::Direct3D11::IDirect3DSurface,
    Win32::{
        Graphics::Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        },
        System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess,
    },
};

/// Pixels Color Representation
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[repr(C)]
pub struct RGBA {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Frame Struct Used To Crop And Get The Frame Buffer
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct Frame<'a> {
    surface: &'a IDirect3DSurface,
    d3d_device: &'a ID3D11Device,
    context: &'a ID3D11DeviceContext,
    width: i32,
    height: i32,
}

impl<'a> Frame<'a> {
    /// Craete A New Frame
    pub const fn new(
        surface: &'a IDirect3DSurface,
        d3d_device: &'a ID3D11Device,
        context: &'a ID3D11DeviceContext,
        width: i32,
        height: i32,
    ) -> Self {
        Self {
            surface,
            d3d_device,
            context,
            width,
            height,
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

        // Create A Copy Texture To Process On
        let mut texture_copy = None;
        unsafe {
            self.d3d_device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture_copy))?
        };
        let texture_copy = texture_copy.unwrap();

        // Copy The Real Texture To Copy Texture
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

        unsafe { self.context.Unmap(&texture_copy, 0) };

        Ok(FrameBuffer {
            slice,
            width: texture_desc.Width,
            height: texture_desc.Height,
        })
    }

    /// Get Part Of The Frame Buffer
    pub fn sub_buffer(
        &self,
        start_width: u32,
        start_height: u32,
        width: u32,
        height: u32,
    ) -> Result<FrameBuffer, Box<dyn std::error::Error>> {
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

        // Create A Copy Texture To Process On
        let mut texture_copy = None;
        unsafe {
            self.d3d_device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture_copy))?
        };
        let texture_copy = texture_copy.unwrap();

        // Create Box Settings
        println!(
            "start_width: {start_width}, start_height: {start_height}, width: {width}, height: \
             {height}"
        );
        let src_box = D3D11_BOX {
            left: start_width,
            top: start_height,
            front: 0,
            right: start_width + width,
            bottom: start_height + height,
            back: 1,
        };

        // Copy The Real Texture To Copy Texture
        unsafe {
            self.context.CopySubresourceRegion(
                &texture_copy,
                0,
                0,
                0,
                0,
                &texture,
                0,
                Some(&src_box),
            )
        };

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

        unsafe { self.context.Unmap(&texture_copy, 0) };

        Ok(FrameBuffer {
            slice,
            width: texture_desc.Width,
            height: texture_desc.Height,
        })
    }

    /// Get Frame Width
    pub const fn width(&self) -> i32 {
        self.width
    }

    /// Get Frame Height
    pub const fn height(&self) -> i32 {
        self.height
    }
}

/// FrameBuffer Struct Used To Crop And Get The Buffer
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
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

    /// Get The Frame Buffer
    pub const fn pixels(&self) -> &'a [RGBA] {
        let pixel_slice: &[RGBA] = unsafe {
            std::slice::from_raw_parts(
                self.slice.as_ptr() as *const RGBA,
                self.slice.len() / std::mem::size_of::<RGBA>(),
            )
        };

        pixel_slice
    }

    /// Get Buffer Width
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Get Buffer Height
    pub const fn height(&self) -> u32 {
        self.height
    }
}
