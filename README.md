# Windows Capture &emsp; [![Licence]][Licence URL] [![Build Status]][repository] [![Latest Version]][crates.io]

[Licence]: https://img.shields.io/crates/l/windows-capture
[Licence URL]: https://github.com/NiiightmareXD/windows-capture/blob/main/LICENCE

[Build Status]: https://img.shields.io/github/actions/workflow/status/NiiightmareXD/windows-capture/rust.yml
[repository]: https://github.com/NiiightmareXD/windows-capture

[Latest Version]: https://img.shields.io/crates/v/windows-capture
[crates.io]: https://crates.io/crates/windows-capture

**Windows Capture** is a highly efficient Rust and Python library that enables you to capture the screen using the Graphics Capture API effortlessly. This library allows you to easily capture the screen of your Windows-based computer and use it for various purposes, such as creating instructional videos, taking screenshots, or recording your gameplay. With its intuitive interface and robust functionality, Windows Capture is an excellent choice for anyone looking for a reliable, easy-to-use screen-capturing solution.

**Note** this README.md is for [Rust library](https://github.com/NiiightmareXD/windows-capture) Python library can be found [here](https://github.com/NiiightmareXD/windows-capture/tree/main/windows-capture-python)

## Features

- Only Updates The Frame When Required.
- High Performance.
- Easy To Use.
- Latest Screen Capturing API.

## Installation

Add this library to your `Cargo.toml`:

```toml
[dependencies]
windows-capture = "1.0.60"
```
or run this command

```
cargo add windows-capture
```

## Usage

```rust
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
```

## Documentation

Detailed documentation for each API and type can be found [here](https://docs.rs/windows-capture).

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request.

## License

This project is licensed under the [MIT License](LICENSE).
