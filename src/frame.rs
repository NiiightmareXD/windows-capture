use std::{
    fs::{self},
    io,
    path::Path,
    ptr, slice,
};

use log::trace;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use windows::{
    Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat},
    Storage::Streams::{Buffer, DataReader, InMemoryRandomAccessStream, InputStreamOptions},
    Win32::Graphics::{
        Direct3D11::{
            ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ,
            D3D11_CPU_ACCESS_WRITE, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ_WRITE,
            D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
        },
        Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC},
    },
};

use crate::settings::ColorFormat;

/// Used To Handle Frame Errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid Box Size")]
    InvalidSize,
    #[error("Invalid Path")]
    InvalidPath,
    #[error("This Color Format Is Not Supported For Saving As Image")]
    UnsupportedFormat,
    #[error(transparent)]
    WindowsError(#[from] windows::core::Error),
    #[error(transparent)]
    IoError(#[from] io::Error),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    Tiff,
    Bmp,
    JpegXr,
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
    pub fn buffer(&mut self) -> Result<FrameBuffer, Error> {
        // Texture Settings
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: self.width,
            Height: self.height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(self.color_format as i32),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32 | D3D11_CPU_ACCESS_WRITE.0 as u32,
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
            self.context.Map(
                &texture,
                0,
                D3D11_MAP_READ_WRITE,
                0,
                Some(&mut mapped_resource),
            )?;
        };

        // Get The Mapped Resource Data Slice
        let mapped_frame_data = unsafe {
            slice::from_raw_parts_mut(
                mapped_resource.pData.cast(),
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
    ) -> Result<FrameBuffer, Error> {
        if start_width >= end_width || start_height >= end_height {
            return Err(Error::InvalidSize);
        }

        let texture_width = end_width - start_width;
        let texture_height = end_height - start_height;

        // Texture Settings
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: texture_width,
            Height: texture_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT(self.color_format as i32),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32 | D3D11_CPU_ACCESS_WRITE.0 as u32,
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
            self.context.Map(
                &texture,
                0,
                D3D11_MAP_READ_WRITE,
                0,
                Some(&mut mapped_resource),
            )?;
        };

        // Get The Mapped Resource Data Slice
        let mapped_frame_data = unsafe {
            slice::from_raw_parts_mut(
                mapped_resource.pData.cast(),
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
        format: ImageFormat,
    ) -> Result<(), Error> {
        let frame_buffer = self.buffer()?;

        frame_buffer.save_as_image(path, format)?;

        Ok(())
    }
}

/// Frame Buffer Struct Used To Get Raw Pixel Data
#[allow(clippy::module_name_repetitions)]
pub struct FrameBuffer<'a> {
    raw_buffer: &'a mut [u8],
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
        raw_buffer: &'a mut [u8],
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
    pub fn as_raw_buffer(&'a mut self) -> &'a mut [u8] {
        self.raw_buffer
    }

    /// Get The Raw Pixel Data Without Padding
    #[allow(clippy::type_complexity)]
    pub fn as_raw_nopadding_buffer(&'a mut self) -> Result<&'a mut [u8], Error> {
        if !self.has_padding() {
            return Ok(self.raw_buffer);
        }

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

        Ok(&mut self.buffer[0..frame_size])
    }

    /// Save The Frame Buffer As An Image To The Specified Path (Only `ColorFormat::Rgba8` And `ColorFormat::Bgra8`)
    pub fn save_as_image<T: AsRef<Path>>(&self, path: T, format: ImageFormat) -> Result<(), Error> {
        let encoder = match format {
            ImageFormat::Jpeg => BitmapEncoder::JpegEncoderId()?,
            ImageFormat::Png => BitmapEncoder::PngEncoderId()?,
            ImageFormat::Gif => BitmapEncoder::GifEncoderId()?,
            ImageFormat::Tiff => BitmapEncoder::TiffEncoderId()?,
            ImageFormat::Bmp => BitmapEncoder::BmpEncoderId()?,
            ImageFormat::JpegXr => BitmapEncoder::JpegXREncoderId()?,
        };

        let stream = InMemoryRandomAccessStream::new()?;
        let encoder = BitmapEncoder::CreateAsync(encoder, &stream)?.get()?;

        let pixelformat = match self.color_format {
            ColorFormat::Bgra8 => BitmapPixelFormat::Bgra8,
            ColorFormat::Rgba8 => BitmapPixelFormat::Rgba8,
            ColorFormat::Rgba16F => return Err(Error::UnsupportedFormat),
        };

        encoder.SetPixelData(
            pixelformat,
            BitmapAlphaMode::Premultiplied,
            self.width,
            self.height,
            1.0,
            1.0,
            self.raw_buffer,
        )?;

        encoder.FlushAsync()?.get()?;

        let buffer = Buffer::Create(u32::try_from(stream.Size()?).unwrap())?;
        stream
            .ReadAsync(&buffer, buffer.Capacity()?, InputStreamOptions::None)?
            .get()?;

        let data_reader = DataReader::FromBuffer(&buffer)?;
        let length = data_reader.UnconsumedBufferLength()?;
        let mut bytes = vec![0u8; length as usize];
        data_reader.ReadBytes(&mut bytes).unwrap();

        fs::write(path, bytes)?;

        Ok(())
    }
}
