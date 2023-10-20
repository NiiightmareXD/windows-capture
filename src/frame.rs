use std::{mem, ptr};

use image::ColorType;
use thiserror::Error;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext, ID3D11Texture2D, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
};

/// Used To Handle Internal Frame Errors
#[derive(Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum FrameError {
    #[error("Graphics Capture API Is Not Supported")]
    InvalidSize,
}

/// Pixels Color Representation
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[repr(C)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// Frame Struct Used To Crop And Get The Frame Buffer
pub struct Frame {
    buffer: *mut u8,
    texture: ID3D11Texture2D,
    frame_surface: ID3D11Texture2D,
    context: ID3D11DeviceContext,
    width: u32,
    height: u32,
}

impl Frame {
    /// Craete A New Frame
    pub const fn new(
        buffer: *mut u8,
        texture: ID3D11Texture2D,
        frame_surface: ID3D11Texture2D,
        context: ID3D11DeviceContext,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            texture,
            frame_surface,
            context,
            width,
            height,
            buffer,
        }
    }

    /// Get The Frame Width
    pub fn width(&self) -> u32 {
        self.width
    }

    // Get The Frame Height
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get The Frame Buffer
    pub fn buffer(&self) -> Result<&[Rgba], Box<dyn std::error::Error>> {
        // Copy The Real Texture To Copy Texture
        unsafe {
            self.context
                .CopyResource(&self.texture, &self.frame_surface)
        };

        // Map The Texture To Enable CPU Access
        let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context.Map(
                &self.texture,
                0,
                D3D11_MAP_READ,
                0,
                Some(&mut mapped_resource),
            )?
        };

        // Create A Slice From The Bits
        let slice = if self.width * 4 == mapped_resource.RowPitch {
            // Means There Is No Padding And We Can Do Our Work
            unsafe {
                std::slice::from_raw_parts(
                    mapped_resource.pData as *const Rgba,
                    (self.height * mapped_resource.RowPitch) as usize / std::mem::size_of::<Rgba>(),
                )
            }
        } else {
            // There Is Padding So We Have To Work According To:
            // https://learn.microsoft.com/en-us/windows/win32/medfound/image-stride
            let row_size = self.width as usize * std::mem::size_of::<Rgba>();

            for i in 0..self.height {
                unsafe {
                    ptr::copy_nonoverlapping(
                        mapped_resource
                            .pData
                            .add((i * mapped_resource.RowPitch) as usize)
                            as *mut u8,
                        self.buffer.add(i as usize * row_size),
                        row_size,
                    )
                };
            }

            unsafe {
                std::slice::from_raw_parts(
                    self.buffer as *mut Rgba,
                    (self.width * self.height) as usize,
                )
            }
        };

        Ok(slice)
    }

    // /// Get Part Of The Frame Buffer
    // pub fn sub_buffer(
    //     &self,
    //     start_width: u32,
    //     start_height: u32,
    //     width: u32,
    //     height: u32,
    // ) -> Result<FrameBuffer, Box<dyn std::error::Error>> {
    // }

    /// Save The Frame As An Image To Specified Path
    pub fn save_as_image(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let buffer = self.buffer()?;

        let buf = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr() as *mut u8, mem::size_of_val(buffer))
        };

        image::save_buffer(path, buf, self.width, self.height, ColorType::Rgba8)?;

        Ok(())
    }
}
