use std::alloc::Layout;

/// To Send Raw Pointers Between Threads
pub struct SendSyncPtr<T>(pub *mut T);

impl<T> SendSyncPtr<T> {
    pub const fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }
}

unsafe impl<T> Send for SendSyncPtr<T> {}
unsafe impl<T> Sync for SendSyncPtr<T> {}

/// To Share Buffer Struct Between Threads
pub struct SendBuffer<T>(pub T);

impl<T> SendBuffer<T> {
    pub const fn new(device: T) -> Self {
        Self(device)
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl<T> Send for SendBuffer<T> {}

/// To Save Pointer And It's Layout Together
#[derive(Clone, Copy)]
pub struct Buffer {
    pub ptr: *mut u8,
    pub layout: Layout,
}

impl Buffer {
    pub const fn new(ptr: *mut u8, layout: Layout) -> Self {
        Self { ptr, layout }
    }
}
