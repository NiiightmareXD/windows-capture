use std::{
    io::{self, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use clap::Parser;

use windows_capture::{
    capture::GraphicsCaptureApiHandler,
    encoder::{
        AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
    },
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    monitor::Monitor,
    settings::{ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings},
    window::Window,
};

use windows::Graphics::Capture::GraphicsCaptureItem;

// This struct will be used to handle the capture events.
struct Capture {
    // The video encoder that will be used to encode the frames.
    encoder: Option<VideoEncoder>,
    // To measure the time the capture has been running
    start: Instant,
    // To count the number of frames captured
    frame_count: u64,
    // Flag to check if recording should stop
    stop_flag: Arc<AtomicBool>,
}

impl GraphicsCaptureApiHandler for Capture {
    // The type of flags used to get the values from the settings.
    type Flags = Arc<AtomicBool>;

    // The type of error that can occur during capture, the error will be returned from `CaptureControl` and `start` functions.
    type Error = Box<dyn std::error::Error + Send + Sync>;

    // Function that will be called to create the struct. The flags can be passed from settings.
    fn new(stop_flag: Self::Flags) -> Result<Self, Self::Error> {
        println!("Capture started.");

        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(1920, 1080),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            "video.mp4",
        )?;

        Ok(Self {
            encoder: Some(encoder),
            start: Instant::now(),
            frame_count: 0,
            stop_flag,
        })
    }

    // Called every time a new frame is available.
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        self.frame_count += 1;

        let elapsed_secs = self.start.elapsed().as_secs_f64();
        let fps = self.frame_count as f64 / elapsed_secs;

        print!(
            "\rRecording for: {:.2} seconds | FPS: {:.2}",
            elapsed_secs, fps
        );
        io::stdout().flush()?;

        // Send the frame to the video encoder
        self.encoder.as_mut().unwrap().send_frame(frame)?;

        // Stop the capture if stop_flag is set
        if self.stop_flag.load(Ordering::SeqCst) {
            // Finish the encoder and save the video.
            self.encoder.take().unwrap().finish()?;

            capture_control.stop();

            println!("\nRecording stopped by user.");
        }

        Ok(())
    }

    // Optional handler called when the capture item (usually a window) closes.
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        println!("Capture Session Closed");

        Ok(())
    }
}

#[derive(Parser)]
#[command(name = "Screen Capture")]
#[command(version = "1.0")]
#[command(author = "Your Name")]
#[command(about = "Captures the screen")]
struct Cli {
    /// Window name to capture
    #[arg(long, conflicts_with = "monitor_index")]
    window_name: Option<String>,

    /// Monitor index to capture
    #[arg(long, conflicts_with = "window_name")]
    monitor_index: Option<u32>,

    /// Cursor capture settings: always, never, default
    #[arg(long, default_value = "default")]
    cursor_capture: String,

    /// Draw border settings: always, never, default
    #[arg(long, default_value = "default")]
    draw_border: String,
}

fn parse_cursor_capture(s: &str) -> CursorCaptureSettings {
    match s.to_lowercase().as_str() {
        "always" => CursorCaptureSettings::WithCursor,
        "never" => CursorCaptureSettings::WithoutCursor,
        "default" => CursorCaptureSettings::Default,
        _ => {
            eprintln!("Invalid cursor_capture value: {}", s);
            std::process::exit(1);
        }
    }
}

fn parse_draw_border(s: &str) -> DrawBorderSettings {
    match s.to_lowercase().as_str() {
        "always" => DrawBorderSettings::WithBorder,
        "never" => DrawBorderSettings::WithoutBorder,
        "default" => DrawBorderSettings::Default,
        _ => {
            eprintln!("Invalid draw_border value: {}", s);
            std::process::exit(1);
        }
    }
}

fn start_capture<T>(
    capture_item: T,
    cursor_capture: CursorCaptureSettings,
    draw_border: DrawBorderSettings,
    stop_flag: Arc<AtomicBool>,
) where
    T: TryInto<GraphicsCaptureItem>,
{
    let settings = Settings::new(
        capture_item,
        cursor_capture,
        draw_border,
        ColorFormat::Rgba8,
        stop_flag.clone(),
    );

    // Starts the capture and takes control of the current thread.
    // The errors from handler trait will end up here
    Capture::start(settings).expect("Screen Capture Failed");
}

fn main() {
    let cli = Cli::parse();

    let cursor_capture = parse_cursor_capture(&cli.cursor_capture);
    let draw_border = parse_draw_border(&cli.draw_border);

    let stop_flag = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    {
        let stop_flag = stop_flag.clone();
        ctrlc::set_handler(move || {
            stop_flag.store(true, Ordering::SeqCst);
        })
        .expect("Error setting Ctrl-C handler");
    }

    if let Some(window_name) = cli.window_name {
        let capture_item =
            Window::from_contains_name(&window_name).expect("Window not found!");
        start_capture(capture_item, cursor_capture, draw_border, stop_flag);
    } else if let Some(index) = cli.monitor_index {
        let capture_item =
            Monitor::from_index(usize::try_from(index).unwrap()).expect("Monitor not found!");
        start_capture(capture_item, cursor_capture, draw_border, stop_flag);
    } else {
        eprintln!("Either --window-name or --monitor-index must be provided");
        std::process::exit(1);
    }
}
