use std::{error::Error, path::Path, ptr, slice};

use image::{Rgb, RgbImage};
use log::trace;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
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
    buffer: &'a mut Vec<u8>,
    width: u32,
    height: u32,
    color_format: ColorFormat,
}

impl<'a> Frame<'a> {
    /// Craete A New Frame
    #[must_use]
    pub fn new(
        d3d_device: &'a ID3D11Device,
        frame_surface: ID3D11Texture2D,
        context: &'a ID3D11DeviceContext,
        buffer: &'a mut Vec<u8>,
        width: u32,
        height: u32,
        color_format: ColorFormat,
    ) -> Self {
        Self {
            d3d_device,
            frame_surface,
            context,
            buffer,
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
            self.buffer,
            self.width,
            self.height,
            mapped_resource.RowPitch,
            mapped_resource.DepthPitch,
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
            self.buffer,
            texture_width,
            texture_height,
            mapped_resource.RowPitch,
            mapped_resource.DepthPitch,
            self.color_format,
        );

        Ok(frame_buffer)
    }

    /// Save The Frame Buffer As An Image To The Specified Path
    pub fn save_as_image<T: AsRef<Path>>(
        &mut self,
        path: T,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let frame_buffer = self.buffer()?;

        frame_buffer.save_as_image(path)?;

        Ok(())
    }
}

/// Frame Buffer Struct Used To Get Raw Pixel Data
pub struct FrameBuffer<'a> {
    raw_buffer: &'a [u8],
    buffer: &'a mut Vec<u8>,
    width: u32,
    height: u32,
    row_pitch: u32,
    depth_pitch: u32,
    color_format: ColorFormat,
}

impl<'a> FrameBuffer<'a> {
    /// Create A New Frame Buffer
    #[must_use]
    pub fn new(
        raw_buffer: &'a [u8],
        buffer: &'a mut Vec<u8>,
        width: u32,
        height: u32,
        row_pitch: u32,
        depth_pitch: u32,
        color_format: ColorFormat,
    ) -> Self {
        Self {
            raw_buffer,
            buffer,
            width,
            height,
            row_pitch,
            depth_pitch,
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

    /// Get The Frame Buffer Row Pitch
    #[must_use]
    pub const fn row_pitch(&self) -> u32 {
        self.row_pitch
    }

    /// Get The Frame Buffer Depth Pitch
    #[must_use]
    pub const fn depth_pitch(&self) -> u32 {
        self.depth_pitch
    }

    /// Check If The Buffer Has Padding
    #[must_use]
    pub const fn has_padding(&self) -> bool {
        self.width * 4 != self.row_pitch
    }

    /// Get The Raw Pixel Data Might Have Padding
    #[must_use]
    pub const fn as_raw_buffer(&self) -> &'a [u8] {
        self.raw_buffer
    }

    /// Get The Raw Pixel Data Without Padding
    #[allow(clippy::type_complexity)]
    pub fn as_raw_nopadding_buffer(&'a mut self) -> Result<&'a [u8], Box<dyn Error + Send + Sync>> {
        let frame_size = (self.width * self.height * 4) as usize;
        if self.buffer.capacity() < frame_size {
            trace!("Resizing Preallocated Buffer");
            self.buffer.resize(frame_size, 0);
        }

        let width_size = (self.width * 4) as usize;
        let buffer_address = self.buffer.as_mut_ptr() as isize;
        (0..self.height).into_par_iter().for_each(|y| {
            let index = (y * self.row_pitch) as usize;
            let ptr = buffer_address as *mut u8;

            unsafe {
                ptr::copy_nonoverlapping(
                    self.raw_buffer.as_ptr().add(index),
                    ptr.add(y as usize * width_size),
                    width_size,
                );
            }
        });

        Ok(&self.buffer[0..frame_size])
    }

    /// Save The Frame Buffer As An Image To The Specified Path
    pub fn save_as_image<T: AsRef<Path>>(
        &self,
        path: T,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut rgb_image: RgbImage = RgbImage::new(self.width, self.height);

        if self.color_format == ColorFormat::Rgba8 {
            for y in 0..self.height {
                for x in 0..self.width {
                    let first_index = (y * self.row_pitch + x * 4) as usize;

                    let r = self.raw_buffer[first_index];
                    let g = self.raw_buffer[first_index + 1];
                    let b = self.raw_buffer[first_index + 2];

                    rgb_image.put_pixel(x, y, Rgb([r, g, b]));
                }
            }
        } else {
            for y in 0..self.height {
                for x in 0..self.width {
                    let first_index = (y * self.row_pitch + x * 4) as usize;

                    let b = self.raw_buffer[first_index];
                    let g = self.raw_buffer[first_index + 1];
                    let r = self.raw_buffer[first_index + 2];

                    rgb_image.put_pixel(x, y, Rgb([r, g, b]));
                }
            }
        }

        rgb_image.save(path)?;

        Ok(())
    }
}
