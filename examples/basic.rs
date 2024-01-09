use windows_capture::{
    capture::WindowsCaptureHandler,
    frame::{Frame, ImageFormat},
    graphics_capture_api::InternalCaptureControl,
    settings::{ColorFormat, Settings},
    window::Window,
};

// Struct To Implement The Trait For
struct Capture;

impl WindowsCaptureHandler for Capture {
    // Any Value To Get From The Settings
    type Flags = String;

    // To Redirect To `CaptureControl` Or Start Method
    type Error = Box<dyn std::error::Error + Send + Sync>;

    // Function That Will Be Called To Create The Struct The Flags Can Be Passed
    // From `WindowsCaptureSettings`
    fn new(message: Self::Flags) -> Result<Self, Self::Error> {
        println!("Got The Flag: {message}");

        Ok(Self {})
    }

    // Called Every Time A New Frame Is Available
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        println!("New Frame Arrived");

        // Save The Frame As An Image To The Specified Path
        frame.save_as_image("image.png", ImageFormat::Png)?;

        // Gracefully Stop The Capture Thread
        capture_control.stop();

        Ok(())
    }

    // Called When The Capture Item Closes Usually When The Window Closes, Capture
    // Session Will End After This Function Ends
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        println!("Capture Session Closed");

        Ok(())
    }
}

fn main() {
    // Gets The Foreground Window, Checkout The Docs For Other Capture Items
    let foreground_window = Window::foreground().expect("No Active Window Found");

    let settings = Settings::new(
        // Item To Captue
        foreground_window,
        // Capture Cursor
        Some(true),
        // Draw Borders (None Means Default Api Configuration)
        None,
        // Kind Of Pixel Format For Frame To Have
        ColorFormat::Rgba8,
        // Any Value To Pass To The New Function
        "It Works".to_string(),
    )
    .unwrap();

    // Every Error From `new`, `on_frame_arrived` and `on_closed` Will End Up Here
    Capture::start(settings).expect("Screen Capture Failed");
}
