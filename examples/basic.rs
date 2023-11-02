use windows_capture::{
    capture::WindowsCaptureHandler, frame::Frame, settings::WindowsCaptureSettings, window::Window,
};

struct Capture;

impl WindowsCaptureHandler for Capture {
    type Flags = String; // To Get The Message (Or A Variable Or ...) From The Settings

    fn new(message: Self::Flags) -> Self {
        // Function That Will Be Called To Create The Struct The Flags Can Be Passed
        // From Settings
        println!("Got The Message: {message}");

        Self {}
    }

    fn on_frame_arrived(&mut self, mut frame: Frame) {
        // Called Every Time A New Frame Is Available
        println!("Got A New Frame");

        // Save The Frame As An Image To Specified Path
        frame.save_as_image("image.png").unwrap();

        // Call To Stop The Capture Thread, You Might Receive A Few More Frames
        // Before It Stops
        self.stop();
    }

    fn on_closed(&mut self) {
        // Called When The Capture Item Closes Usually When The Window Closes,
        // Capture Will End After This Function Ends
        println!("Capture Item Closed");
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
        // Draw Border
        Some(false),
        // This Will Be Passed To The New Function
        "It Works".to_string(),
    )
    .unwrap();

    Capture::start(settings).unwrap();
}
