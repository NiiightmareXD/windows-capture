use windows_capture::capture_ext::*;
use windows_capture::graphics_capture_picker::GraphicsCapturePicker;

fn main() {
    GraphicsCapturePicker::pick_item()
        .expect("Failed to pick item")
        .unwrap()
        .start_with_closed_handler(
            &CaptureSettings::default(),
            |frame, control| {
                //frame.save_as_image("target/test.png", ImageFormat::Png);
                //control.stop();
                //Err("Failed to save image")
            },
            || {
                println!("Done!");
                //Ok(())
            },
        )
        .unwrap();
}
