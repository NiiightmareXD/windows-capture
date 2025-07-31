# Real-time Video Streaming Feature

This document describes the new real-time video streaming feature added to the Windows Capture library, which allows you to transmit encoded video frames over the network without writing to files.

## Overview

The streaming feature enables real-time transmission of encoded video frames for live streaming applications, remote desktop solutions, and network-based video processing. It provides a callback-based architecture that allows you to handle encoded frames as they become available.

## Key Components

### 1. StreamingVideoEncoder

The `StreamingVideoEncoder` is the core component that replaces the file-based `VideoEncoder` for streaming applications.

```rust
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, StreamingVideoEncoder, VideoSettingsBuilder,
};

let encoder = StreamingVideoEncoder::new(
    VideoSettingsBuilder::new(1920, 1080),
    AudioSettingsBuilder::default().disabled(true),
    ContainerSettingsBuilder::default(),
    callback, // FrameCallback implementation
)?;
```

### 2. FrameCallback Trait

The `FrameCallback` trait defines the interface for handling encoded frames in real-time:

```rust
use windows_capture::encoder::{EncodedFrame, EncodedAudioFrame, FrameCallback};

pub trait FrameCallback: Send + Sync {
    fn on_video_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn on_audio_frame(&mut self, frame: EncodedAudioFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn on_stream_start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn on_stream_end(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}
```

### 3. EncodedFrame and EncodedAudioFrame

These structs contain the encoded frame data with metadata:

```rust
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub data: Vec<u8>,           // Raw encoded frame data
    pub timestamp: i64,           // Timestamp in 100-nanosecond units
    pub frame_type: FrameType,    // Key frame, delta frame, etc.
    pub width: u32,              // Original frame width
    pub height: u32,             // Original frame height
}

#[derive(Debug, Clone)]
pub struct EncodedAudioFrame {
    pub data: Vec<u8>,           // Raw encoded audio data
    pub timestamp: i64,           // Timestamp in 100-nanosecond units
    pub sample_count: u32,        // Number of audio samples
}
```

## Network Transmission

### Supported Protocols

1. **TCP**: Reliable transmission for local network streaming
2. **UDP**: Fast transmission with potential packet loss
3. **File**: Save encoded frames to files for debugging
4. **WebRTC**: Real-time communication (planned)
5. **RTMP**: Streaming protocol (planned)

### NetworkCallback

The `NetworkCallback` provides built-in network transmission capabilities:

```rust
use windows_capture::network::{NetworkCallback, NetworkConfig, Protocol};

let config = NetworkConfig {
    protocol: Protocol::Tcp,
    address: "127.0.0.1:8080".to_string(),
    frame_rate: 30,
    ..Default::default()
};

let callback = Box::new(NetworkCallback::new(config)?);
```

### FileCallback

For debugging and testing, use the `FileCallback` to save encoded frames:

```rust
use windows_capture::network::FileCallback;

let callback = Box::new(FileCallback::new("output_directory".to_string()));
```

## Usage Examples

### Basic Streaming Example

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

        // Create network callback
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
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        self.encoder.as_mut().unwrap().send_frame(frame)?;
        Ok(())
    }
}
```

### Custom Callback Implementation

You can implement your own `FrameCallback` for custom processing:

```rust
use windows_capture::encoder::{EncodedFrame, EncodedAudioFrame, FrameCallback};

struct CustomCallback {
    frame_count: u64,
}

impl FrameCallback for CustomCallback {
    fn on_video_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Received video frame {}: {} bytes", self.frame_count, frame.data.len());
        self.frame_count += 1;
        
        // Your custom processing here
        // e.g., send to WebSocket, process with AI, etc.
        
        Ok(())
    }

    fn on_audio_frame(&mut self, frame: EncodedAudioFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Received audio frame: {} bytes", frame.data.len());
        Ok(())
    }

    fn on_stream_start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Stream started");
        Ok(())
    }

    fn on_stream_end(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Stream ended after {} frames", self.frame_count);
        Ok(())
    }
}
```

## Running the Examples

### Streaming Example

```bash
# Stream to TCP server
cargo run --example streaming tcp

# Stream to UDP client
cargo run --example streaming udp

# Save encoded frames to files
cargo run --example streaming file
```

### Command Line Arguments

The streaming example accepts a protocol argument:

- `tcp`: Stream to TCP server on 127.0.0.1:8080
- `udp`: Stream to UDP client on 127.0.0.1:8080
- `file`: Save encoded frames to `streaming_output/` directory

## Configuration Options

### NetworkConfig

```rust
pub struct NetworkConfig {
    pub protocol: Protocol,        // Transmission protocol
    pub address: String,           // Target address
    pub max_buffer_size: usize,    // Maximum frame buffer size
    pub frame_rate: u32,          // Target frame rate
    pub quality: Quality,         // Quality settings
}
```

### Quality Settings

```rust
pub struct Quality {
    pub video_bitrate: u32,       // Video bitrate in bits per second
    pub audio_bitrate: u32,       // Audio bitrate in bits per second
    pub max_frame_size: usize,    // Maximum frame size in bytes
}
```

## Performance Considerations

1. **Frame Rate**: Set appropriate frame rates based on network capacity
2. **Bitrate**: Adjust quality settings for your network conditions
3. **Buffer Size**: Configure buffer sizes to balance latency and reliability
4. **Protocol Choice**: Use TCP for reliability, UDP for low latency

## Error Handling

The streaming implementation includes comprehensive error handling:

- Network connection failures
- Frame encoding errors
- Callback processing errors
- Resource cleanup

## Future Enhancements

1. **WebRTC Support**: Real-time communication protocol
2. **RTMP Support**: Streaming protocol for platforms like YouTube
3. **HLS/DASH Support**: Adaptive bitrate streaming
4. **Hardware Acceleration**: GPU-accelerated encoding
5. **Multi-stream Support**: Multiple concurrent streams

## Migration from File-based Encoding

To migrate from the file-based `VideoEncoder` to the streaming `StreamingVideoEncoder`:

1. Replace `VideoEncoder::new()` with `StreamingVideoEncoder::new()`
2. Implement or use a `FrameCallback`
3. Remove file path parameters
4. Handle encoded frames in your callback

## Troubleshooting

### Common Issues

1. **Network Connection Failed**: Check firewall settings and port availability
2. **High Latency**: Reduce frame rate or use UDP protocol
3. **Frame Drops**: Increase buffer size or reduce quality settings
4. **Memory Usage**: Monitor callback processing time

### Debug Mode

Use the `FileCallback` for debugging:

```rust
let callback = Box::new(FileCallback::new("debug_output".to_string()));
```

This will save encoded frames to files for analysis.

## API Reference

### StreamingVideoEncoder

- `new()`: Create a new streaming encoder
- `send_frame()`: Send a video frame for encoding
- `send_frame_with_audio()`: Send a video frame with audio
- `finish()`: Complete encoding and cleanup

### FrameCallback

- `on_video_frame()`: Handle encoded video frames
- `on_audio_frame()`: Handle encoded audio frames
- `on_stream_start()`: Called when streaming begins
- `on_stream_end()`: Called when streaming ends

### NetworkCallback

- `new()`: Create network callback with configuration
- `start()`: Start network services

### FileCallback

- `new()`: Create file callback with output directory

## Contributing

When contributing to the streaming feature:

1. Follow the existing code style
2. Add comprehensive tests
3. Update documentation
4. Consider performance implications
5. Test with different network conditions