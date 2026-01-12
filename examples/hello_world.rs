use windows_capture::capture_ext::*;
use windows_capture::encoder::ImageFormat;
use windows_capture::window::Window;

fn main() {
    let image_path = "target/frame.png";
    //let item = GraphicsCapturePicker::pick_item().unwrap().unwrap();
    //let item=Monitor::primary().unwrap();
    let item = Window::foreground().unwrap();
    item.start(Default::default(), move |p| {
        if let Some((frame, handle)) = p {
            frame.save_as_image(image_path, ImageFormat::Png).unwrap();
            println!("Saved frame to {}", image_path);
            handle.stop();
        } else {
            eprintln!("Capture failed: Item closed");
        }
        Ok::<(), ()>(())
    })
    .unwrap();
}
