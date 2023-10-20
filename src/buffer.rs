use std::alloc::Layout;

/// To Send Raw Pointers Between Threads
pub struct SendPtr<T>(pub *mut T);

impl<T> SendPtr<T> {
    pub fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }
}

unsafe impl<T> Send for SendPtr<T> {}

/// To Save Pointer And It's Layout Together
pub struct Buffer {
    pub ptr: *mut u8,
    pub layout: Layout,
}

impl Buffer {
    pub const fn new(ptr: *mut u8, layout: Layout) -> Self {
        Self { ptr, layout }
    }
}
