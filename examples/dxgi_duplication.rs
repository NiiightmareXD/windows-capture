use windows_capture::dxgi_duplication_api::{DxgiDuplicationApi, Error as DupError};
use windows_capture::frame::ImageFormat;
use windows_capture::monitor::Monitor;

fn main() {
    // Capture the primary monitor using DXGI duplication API
    let monitor = Monitor::primary().expect("No primary monitor found");

    // Create a new DXGI duplication API instance
    let mut dxgi = DxgiDuplicationApi::new(monitor).expect("Failed to create DXGI duplication API");

    // Try to acquire the next frame with a timeout of 60 milliseconds
    match dxgi.acquire_next_frame(60) {
        Ok(mut frame) => {
            let mut buf = frame.buffer().expect("Failed to get frame buffer");
            buf.save_as_image("dxgi_duplication_capture.png", ImageFormat::Png).expect("Failed to save image");
            println!("Saved dxgi_duplication_capture.png ({}x{})", buf.width(), buf.height());
        }
        Err(DupError::FrameTimeout) => {
            eprintln!("No new frame available within the timeout period")
        }
        Err(DupError::AccessLost) => {
            eprintln!("Duplication access lost")
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
