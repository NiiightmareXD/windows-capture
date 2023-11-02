/// To Send Raw Pointers Between Threads
pub struct SendSyncPtr<T>(pub *mut T);

impl<T> SendSyncPtr<T> {
    pub const fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }
}

unsafe impl<T> Send for SendSyncPtr<T> {}
unsafe impl<T> Sync for SendSyncPtr<T> {}

/// To Send Raw Pointers Between Threads
pub struct SendPtr<T>(pub *mut T);

impl<T> SendPtr<T> {
    pub const fn new(ptr: *mut T) -> Self {
        Self(ptr)
    }
}

unsafe impl<T> Send for SendPtr<T> {}
