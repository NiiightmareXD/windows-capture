use std::{error::Error, path::Path, slice};

use image::{Rgba, RgbaImage};
use ndarray::{s, ArrayBase, ArrayView, Dim, OwnedRepr};
use windows::Win32::Graphics::{
    Direct3D11::{
        ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    },
    Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
};

use crate::settings::ColorFormat;

/// Used To Handle Frame Errors
#[derive(thiserror::Error, Eq, PartialEq, Clone, Copy, Debug)]
pub enum FrameError {
    #[error("Invalid Box Size")]
    InvalidSize,
}

/// Frame Struct Used To Get The Frame Buffer
pub struct Frame<'a> {
    d3d_device: &'a ID3D11Device,
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
        d3d_device: &'a ID3D11Device,
        frame_surface: ID3D11Texture2D,
        context: &'a ID3D11DeviceContext,
        width: u32,
        height: u32,
        color_format: ColorFormat,
    ) -> Self {
        Self {
            d3d_device,
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
        // Texture Settings
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: if self.color_format == ColorFormat::Rgba8 {
                DXGI_FORMAT_R8G8B8A8_UNORM
            } else {
                DXGI_FORMAT_B8G8R8A8_UNORM
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        // Create A Texture That CPU Can Read
        let mut texture = None;
        unsafe {
            self.d3d_device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture))?;
        };
        let texture = texture.unwrap();

        // Copy The Real Texture To Copy Texture
        unsafe {
            self.context.CopyResource(&texture, &self.frame_surface);
        };

        // Map The Texture To Enable CPU Access
        let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped_resource))?;
        };

        // Get The Mapped Resource Data Slice
        let mapped_frame_data = unsafe {
            slice::from_raw_parts(
                mapped_resource.pData as *const u8,
                (self.height * mapped_resource.RowPitch) as usize,
            )
        };

        // Create Frame Buffer From Slice
        let frame_buffer = FrameBuffer::new(
            mapped_frame_data,
            self.width,
            self.height,
            self.color_format,
        );

        Ok(frame_buffer)
    }

    /// Get A Cropped Frame Buffer
    pub fn buffer_crop(
        &mut self,
        start_width: u32,
        start_height: u32,
        end_width: u32,
        end_height: u32,
    ) -> Result<FrameBuffer, Box<dyn Error + Send + Sync>> {
        if start_width >= end_width || start_height >= end_height {
            return Err(Box::new(FrameError::InvalidSize));
        }

        let texture_width = end_width - start_width;
        let texture_height = end_height - start_height;

        // Texture Settings
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: texture_width,
            Height: texture_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: if self.color_format == ColorFormat::Rgba8 {
                DXGI_FORMAT_R8G8B8A8_UNORM
            } else {
                DXGI_FORMAT_B8G8R8A8_UNORM
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        // Create A Texture That CPU Can Read
        let mut texture = None;
        unsafe {
            self.d3d_device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture))?;
        };
        let texture = texture.unwrap();

        // Box Settings
        let resource_box = D3D11_BOX {
            left: start_width,
            top: start_height,
            front: 0,
            right: end_width,
            bottom: end_height,
            back: 1,
        };

        // Copy The Real Texture To Copy Texture
        unsafe {
            self.context.CopySubresourceRegion(
                &texture,
                0,
                0,
                0,
                0,
                &self.frame_surface,
                0,
                Some(&resource_box),
            );
        };

        // Map The Texture To Enable CPU Access
        let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped_resource))?;
        };

        // Get The Mapped Resource Data Slice
        let mapped_frame_data = unsafe {
            slice::from_raw_parts(
                mapped_resource.pData as *const u8,
                (texture_height * mapped_resource.RowPitch) as usize,
            )
        };

        // Create Frame Buffer From Slice
        let frame_buffer = FrameBuffer::new(
            mapped_frame_data,
            texture_width,
            texture_height,
            self.color_format,
        );

        Ok(frame_buffer)
    }

    /// Save The Frame As An Image To The Specified Path
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
    color_format: ColorFormat,
}

impl<'a> FrameBuffer<'a> {
    /// Create A New Frame Buffer
    #[must_use]
    pub const fn new(
        raw_buffer: &'a [u8],
        width: u32,
        height: u32,
        color_format: ColorFormat,
    ) -> Self {
        Self {
            raw_buffer,
            width,
            height,
            color_format,
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

    /// Save The Frame Buffer As An Image To The Specified Path
    pub fn save_as_image<T: AsRef<Path>>(
        &self,
        path: T,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let nopadding_buffer = self.as_raw_nopadding_buffer()?;

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
