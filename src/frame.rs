use std::{error::Error, path::Path};

use image::{Rgba, RgbaImage};
use ndarray::{s, ArrayBase, ArrayView, Dim, OwnedRepr};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext, ID3D11Texture2D, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
};

use crate::settings::ColorFormat;

/// Used To Handle Frame Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum FrameError {
    #[error("Graphics Capture API Is Not Supported")]
    InvalidSize,
}

/// Frame Struct Used To Get The Frame Buffer
pub struct Frame<'a> {
    texture: ID3D11Texture2D,
    frame_surface: ID3D11Texture2D,
    context: &'a ID3D11DeviceContext,
    width: u32,
    height: u32,
    color_format: ColorFormat,
}

impl<'a> Frame<'a> {
    /// Craete A New Frame
    #[must_use]
    pub const fn new(
        texture: ID3D11Texture2D,
        frame_surface: ID3D11Texture2D,
        context: &'a ID3D11DeviceContext,
        width: u32,
        height: u32,
        color_format: ColorFormat,
    ) -> Self {
        Self {
            texture,
            frame_surface,
            context,
            width,
            height,
            color_format,
        }
    }

    /// Get The Frame Width
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Get The Frame Height
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Get The Frame Buffer
    pub fn buffer(&mut self) -> Result<FrameBuffer, Box<dyn Error + Send + Sync>> {
        // Copy The Real Texture To Copy Texture
        unsafe {
            self.context
                .CopyResource(&self.texture, &self.frame_surface);
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
            )?;
        };

        // Get The Mapped Resource Data Slice
        let mapped_frame_data = unsafe {
            std::slice::from_raw_parts(
                mapped_resource.pData as *const u8,
                (self.height * mapped_resource.RowPitch) as usize,
            )
        };

        // Create Frame Buffer From Slice
        let frame_buffer = FrameBuffer::new(mapped_frame_data, self.width, self.height);

        Ok(frame_buffer)
    }

    /// Save The Frame As An Image To Specified Path
    pub fn save_as_image<T: AsRef<Path>>(
        &mut self,
        path: T,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let buffer = self.buffer()?;

        let nopadding_buffer = buffer.as_raw_nopadding_buffer()?;

        let (height, width, _) = nopadding_buffer.dim();
        let mut rgba_image: RgbaImage = RgbaImage::new(width as u32, height as u32);

        if self.color_format == ColorFormat::Rgba8 {
            for y in 0..height {
                for x in 0..width {
                    let r = nopadding_buffer[(y, x, 0)];
                    let g = nopadding_buffer[(y, x, 1)];
                    let b = nopadding_buffer[(y, x, 2)];
                    let a = nopadding_buffer[(y, x, 3)];

                    rgba_image.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
                }
            }
        } else {
            for y in 0..height {
                for x in 0..width {
                    let b = nopadding_buffer[(y, x, 0)];
                    let g = nopadding_buffer[(y, x, 1)];
                    let r = nopadding_buffer[(y, x, 2)];
                    let a = nopadding_buffer[(y, x, 3)];

                    rgba_image.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
                }
            }
        }

        rgba_image.save(path)?;

        Ok(())
    }
}

/// Frame Buffer Struct Used To Get Raw Pixel Data
pub struct FrameBuffer<'a> {
    raw_buffer: &'a [u8],
    width: u32,
    height: u32,
}

impl<'a> FrameBuffer<'a> {
    /// Create A New Frame Buffer
    #[must_use]
    pub const fn new(raw_buffer: &'a [u8], width: u32, height: u32) -> Self {
        Self {
            raw_buffer,
            width,
            height,
        }
    }

    /// Get The Frame Buffer Width
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Get The Frame Buffer Height
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Get The Frame Buffer Height
    #[must_use]
    pub const fn has_padding(&self) -> bool {
        let raw_buffer = self.as_raw_buffer();

        self.width as usize * 4 != raw_buffer.len() / self.height as usize
    }

    /// Get The Raw Pixel Data Might Have Padding
    #[must_use]
    pub const fn as_raw_buffer(&self) -> &'a [u8] {
        self.raw_buffer
    }

    /// Get The Raw Pixel Data Without Padding
    #[allow(clippy::type_complexity)]
    pub fn as_raw_nopadding_buffer(
        &self,
    ) -> Result<ArrayBase<OwnedRepr<u8>, Dim<[usize; 3]>>, Box<dyn Error + Send + Sync>> {
        let row_pitch = self.raw_buffer.len() / self.height as usize;

        let array =
            ArrayView::from_shape((self.height as usize, row_pitch), self.raw_buffer)?.to_owned();

        if self.width as usize * 4 == self.raw_buffer.len() / self.height as usize {
            let array = array.into_shape((self.height as usize, self.width as usize, 4))?;

            Ok(array)
        } else {
            let array = array
                .slice_move(s![.., ..self.width as usize * 4])
                .into_shape((self.height as usize, self.width as usize, 4))?;

            Ok(array)
        }
    }
}
