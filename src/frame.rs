use std::{
    alloc::{self, Layout},
    mem, ptr,
};

use image::ColorType;
use log::info;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use thiserror::Error;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11DeviceContext, ID3D11Texture2D, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
};

use crate::buffer::{Buffer, SendSyncPtr};

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
    buffer: Buffer,
    texture: ID3D11Texture2D,
    frame_surface: ID3D11Texture2D,
    context: ID3D11DeviceContext,
    width: u32,
    height: u32,
}

impl Frame {
    /// Craete A New Frame
    #[must_use]
    pub const fn new(
        buffer: Buffer,
        texture: ID3D11Texture2D,
        frame_surface: ID3D11Texture2D,
        context: ID3D11DeviceContext,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            buffer,
            texture,
            frame_surface,
            context,
            width,
            height,
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
    pub fn buffer(&mut self) -> Result<&[Rgba], Box<dyn std::error::Error>> {
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

            // Reallocate If Buffer Is Too Small
            if self.buffer.layout.size() < (self.width * self.height * 4) as usize {
                info!(
                    "Reallocating Buffer Size To {:.1}MB",
                    ((self.width * self.height * 4) as f32 / 1024.0 / 1024.0)
                );

                let new_cap = (self.width * self.height * 4) as usize;
                let new_layout = Layout::array::<u8>(new_cap)?;

                assert!(
                    new_layout.size() <= isize::MAX as usize,
                    "Allocation too large"
                );

                unsafe {
                    let new_ptr =
                        alloc::realloc(self.buffer.ptr, self.buffer.layout, new_layout.size());

                    self.buffer.ptr = if new_ptr.is_null() {
                        alloc::handle_alloc_error(self.buffer.layout)
                    } else {
                        new_ptr
                    };

                    self.buffer.layout = new_layout;
                };
            }

            let row_size = self.width as usize * std::mem::size_of::<Rgba>();
            let send_sync_ptr = SendSyncPtr::new(self.buffer.ptr);
            let send_sync_pdata = SendSyncPtr::new(mapped_resource.pData.cast::<u8>());

            (0..self.height).into_par_iter().for_each(|i| {
                let send_sync_ptr = &send_sync_ptr;
                let send_sync_pdata = &send_sync_pdata;

                unsafe {
                    ptr::copy_nonoverlapping(
                        send_sync_pdata
                            .0
                            .add((i * mapped_resource.RowPitch) as usize),
                        send_sync_ptr.0.add(i as usize * row_size),
                        row_size,
                    );
                };
            });

            unsafe {
                std::slice::from_raw_parts(
                    self.buffer.ptr.cast::<Rgba>(),
                    (self.width * self.height) as usize,
                )
            }
        };

        Ok(slice)
    }

    /// Save The Frame As An Image To Specified Path
    pub fn save_as_image(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let buffer = self.buffer()?;

        let buf = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), mem::size_of_val(buffer))
        };

        image::save_buffer(path, buf, self.width, self.height, ColorType::Rgba8)?;

        Ok(())
    }
}
