//! Fallible event registration for callbacks that must return an HRESULT to WinRT.
//!
//! Bindgen's convenience event methods accept callbacks returning `()`. Capture
//! and encoding callbacks also propagate Windows errors to the event source.

use windows_core::{EventRevoker, IInspectable, Interface, Ref, Result};

use crate::bindings::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, MediaStreamSource, MediaStreamSourceSampleRequestedEventArgs,
    MediaStreamSourceStartingEventArgs, TypedEventHandler,
};

macro_rules! event {
    ($name:ident, $source:ty, $args:ty, $add:ident, $remove:ident) => {
        pub fn $name<F>(source: &$source, handler: F) -> Result<EventRevoker>
        where
            F: Fn(Ref<$source>, Ref<$args>) -> Result<()> + Send + 'static,
        {
            let handler = TypedEventHandler::<$source, $args>::new(handler);
            let mut token = 0;
            unsafe {
                (source.vtable().$add)(source.as_raw(), handler.as_raw(), &mut token).ok()?;
            }
            Ok(EventRevoker::new(source.clone(), token, source.vtable().$remove))
        }
    };
}

event!(closed, GraphicsCaptureItem, IInspectable, Closed, RemoveClosed);
event!(frame_arrived, Direct3D11CaptureFramePool, IInspectable, FrameArrived, RemoveFrameArrived);
event!(starting, MediaStreamSource, MediaStreamSourceStartingEventArgs, Starting, RemoveStarting);
event!(
    sample_requested,
    MediaStreamSource,
    MediaStreamSourceSampleRequestedEventArgs,
    SampleRequested,
    RemoveSampleRequested
);
