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
    slice: &'a [u8],
    width: u32,
    height: u32,
}

impl<'a> Frame<'a> {
    /// Create A New Frame
    pub const fn new(slice: &'a [u8], width: u32, height: u32) -> Self {
        Self {
            slice,
            width,
            height,
        }
    }

    /// Get The Cropped Version Of The Frame
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

    /// Get The Frame
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
