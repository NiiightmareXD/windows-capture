use windows_capture::capture_ext::*;
use windows_capture::graphics_capture_picker::GraphicsCapturePicker;

fn main() {
    let item = GraphicsCapturePicker::pick_item().expect("Failed to pick item").unwrap();
    item.start(&CaptureSettings::default(), |frame, control| -> Result<(), ()> {
        //println!("Frame: {}", frame);
        Ok(())
    })
    .unwrap();
}
