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

- Only updates the frame when required.
- High performance.
- Easy to use.
- Latest Screen Capturing API.

## Installation

Add this dependency to your `Cargo.toml`:

```toml
[dependencies]
windows-capture = "1.5.0"
```

or run this command

```
cargo add windows-capture
```

## Usage

```rust
use std::io::{self, Write};
use std::time::Instant;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

// Handles capture events.
struct Capture {
    // The video encoder that will be used to encode the frames.
    encoder: Option<VideoEncoder>,
    // To measure the time the capture has been running
    start: Instant,
}

impl GraphicsCaptureApiHandler for Capture {
    // The type of flags used to get the values from the settings.
    type Flags = String;

    // The type of error that can be returned from `CaptureControl` and `start`
    // functions.
    type Error = Box<dyn std::error::Error + Send + Sync>;

    // Function that will be called to create a new instance. The flags can be
    // passed from settings.
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        println!("Created with Flags: {}", ctx.flags);

        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(1920, 1080),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            "video.mp4",
        )?;

        Ok(Self { encoder: Some(encoder), start: Instant::now() })
    }

    // Called every time a new frame is available.
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        print!("\rRecording for: {} seconds", self.start.elapsed().as_secs());
        io::stdout().flush()?;

        // Send the frame to the video encoder
        self.encoder.as_mut().unwrap().send_frame(frame)?;

        // Note: The frame has other uses too, for example, you can save a single frame
        // to a file, like this: frame.save_as_image("frame.png", ImageFormat::Png)?;
        // Or get the raw data like this so you have full
        // control: let data = frame.buffer()?;

        // Stop the capture after 6 seconds
        if self.start.elapsed().as_secs() >= 6 {
            // Finish the encoder and save the video.
            self.encoder.take().unwrap().finish()?;

            capture_control.stop();

            // Because the previous prints did not include a newline.
            println!();
        }

        Ok(())
    }

    // Optional handler called when the capture item (usually a window) is closed.
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        println!("Capture session ended");

        Ok(())
    }
}

fn main() {
    // Gets the primary monitor, refer to the docs for other capture items.
    let primary_monitor = Monitor::primary().expect("There is no primary monitor");

    let settings = Settings::new(
        // Item to capture
        primary_monitor,
        // Capture cursor settings
        CursorCaptureSettings::Default,
        // Draw border settings
        DrawBorderSettings::Default,
        // Secondary window settings, if you want to include secondary windows in the capture
        SecondaryWindowSettings::Default,
        // Minimum update interval, if you want to change the frame rate limit (default is 60 FPS or 16.67 ms)
        MinimumUpdateIntervalSettings::Default,
        // Dirty region settings,
        DirtyRegionSettings::Default,
        // The desired color format for the captured frame.
        ColorFormat::Rgba8,
        // Additional flags for the capture settings that will be passed to the user-defined `new` function.
        "Yea this works".to_string(),
    );

    // Starts the capture and takes control of the current thread.
    // The errors from the handler trait will end up here.
    Capture::start(settings).expect("Screen capture failed");
}
```

## Real-time Streaming

Windows Capture now supports real-time video streaming without writing to files! This feature allows you to transmit encoded video frames over the network for live streaming applications.

### Streaming Example

```rust
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, StreamingVideoEncoder, VideoSettingsBuilder,
};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::network::{NetworkCallback, NetworkConfig, Protocol};
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

struct StreamingCapture {
    encoder: Option<StreamingVideoEncoder>,
    start: std::time::Instant,
}

impl GraphicsCaptureApiHandler for StreamingCapture {
    type Flags = String;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let monitor = Monitor::primary()?;
        let width = monitor.width()?;
        let height = monitor.height()?;

        let video_settings = VideoSettingsBuilder::new(width, height);
        let audio_settings = AudioSettingsBuilder::default().disabled(true);
        let container_settings = ContainerSettingsBuilder::default();

        // Create network callback for TCP streaming
        let config = NetworkConfig {
            protocol: Protocol::Tcp,
            address: "127.0.0.1:8080".to_string(),
            frame_rate: 30,
            ..Default::default()
        };
        let callback = Box::new(NetworkCallback::new(config)?);

        let encoder = StreamingVideoEncoder::new(
            video_settings,
            audio_settings,
            container_settings,
            callback,
        )?;

        Ok(Self {
            encoder: Some(encoder),
            start: std::time::Instant::now(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Send the frame to the streaming encoder
        self.encoder.as_mut().unwrap().send_frame(frame)?;
        Ok(())
    }
}
```

### Supported Protocols

- **TCP**: Reliable transmission for local network streaming
- **UDP**: Fast transmission with potential packet loss
- **File**: Save encoded frames to files for debugging
- **WebRTC**: Real-time communication (planned)
- **RTMP**: Streaming protocol (planned)

### Running the Streaming Example

```bash
# Stream to TCP server
cargo run --example streaming tcp

# Stream to UDP client
cargo run --example streaming udp

# Save encoded frames to files
cargo run --example streaming file
```

## Documentation

Detailed documentation for each API and type can be found [here](https://docs.rs/windows-capture).

## Contributing

Contributions are welcome! If you find a bug or want to add new features to the library, please open an issue or submit a pull request. (also add emojis to commit message 😅)

## License

This project is licensed under the [MIT License](LICENSE).
