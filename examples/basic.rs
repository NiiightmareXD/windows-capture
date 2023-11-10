use std::error::Error;

use windows_capture::{
    capture::WindowsCaptureHandler,
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::{ColorFormat, WindowsCaptureSettings},
    window::Window,
};

// Struct To Implement Methods For
struct Capture;

impl WindowsCaptureHandler for Capture {
    type Flags = String; // To Get The Message From The Settings

    // Function That Will Be Called To Create The Struct The Flags Can Be Passed
    // From Settings
    fn new(message: Self::Flags) -> Result<Self, Box<dyn Error + Send + Sync>> {
        println!("Got The Message: {message}");

        Ok(Self {})
    }

    // Called Every Time A New Frame Is Available
    fn on_frame_arrived(
        &mut self,
        mut frame: Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("New Frame Arrived");

        // Save The Frame As An Image To The Specified Path
        frame.save_as_image("image.png")?;

        // Gracefully Stop The Capture Thread
        capture_control.stop();

        Ok(())
    }

    // Called When The Capture Item Closes Usually When The Window Closes, Capture
    // Session Will End After This Function Ends
    fn on_closed(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        println!("Capture Session Closed");

        Ok(())
    }
}

fn main() {
    // Checkout Docs For Other Capture Items
    let foreground_window = Window::foreground().unwrap();

    let settings = WindowsCaptureSettings::new(
        // Item To Captue
        foreground_window,
        // Capture Cursor
        Some(true),
        // Draw Borders
        Some(false),
        // Kind Of Pixel Format For Frame To Have
        ColorFormat::Rgba8,
        // Will Be Passed To The New Function
        "It Works".to_string(),
    )
    .unwrap();

    // Every Error From on_closed and on_frame_arrived Will End Up Here
    Capture::start(settings).unwrap();
}
