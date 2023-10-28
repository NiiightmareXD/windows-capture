# Windows Capture
![Crates.io](https://img.shields.io/crates/l/windows-capture) ![GitHub Workflow Status (with event)](https://img.shields.io/github/actions/workflow/status/NiiightmareXD/windows-capture/rust.yml) ![Crates.io](https://img.shields.io/crates/v/windows-capture)

**Windows Capture** is a highly efficient Rust and Python library that enables you to effortlessly capture the screen using the Graphics Capture API. This library allows you to easily capture the screen of your Windows-based computer and use it for various purposes, such as creating instructional videos, taking screenshots, or recording your gameplay. With its intuitive interface and robust functionality, Windows-Capture is an excellent choice for anyone looking for a reliable and easy-to-use screen capturing solution.

## Features

- Only Updates The Frame When Required.
- High Performance.
- Easy To Use.
- Latest Screen Capturing API.

## Installation

Add this library to your `Cargo.toml`:

```toml
[dependencies]
windows-capture = "1.0.21"
```
or run this command

```
cargo add windows-capture
```

## Usage

```rust
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

    fn on_frame_arrived(&mut self, frame: Frame) {
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
        // Draw Borders
        Some(false),
        // This Will Be Passed To The New Function
        "It Works".to_string(),
    )
    .unwrap();

    Capture::start(settings).unwrap();
}
```

## Documentation

Detailed documentation for each API and type can be found [here](https://docs.rs/windows-capture).

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENSE).