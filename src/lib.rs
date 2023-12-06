//! # Windows Capture Rust Library
//!
//! **Windows Capture** is a highly efficient Rust and Python library that
//! enables you to effortlessly capture the screen using the Graphics Capture
//! API. This library allows you to easily capture the screen of your
//! Windows-based computer and use it for various purposes, such as creating
//! instructional videos, taking screenshots, or recording your gameplay. With
//! its intuitive interface and robust functionality, Windows-Capture is an
//! excellent choice for anyone looking for a reliable and easy-to-use screen
//! capturing solution.
//!
//! ## Features
//!
//! - Only Updates The Frame When Required.
//! - High Performance.
//! - Easy To Use.
//! - Latest Screen Capturing API.
//!
//! ## Installation
//!
//! Add this library to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! windows-capture = "1.0.51"
//! ```
//! or run this command
//!
//! ```text
//! cargo add windows-capture
//! ```
//!
//! ## Usage
//!
//! ```no_run
//! use windows_capture::{
//!     capture::WindowsCaptureHandler,
//!     frame::Frame,
//!     graphics_capture_api::InternalCaptureControl,
//!     settings::{ColorFormat, Settings},
//!     window::Window,
//! };
//!
//! // Struct To Implement The Trait For
//! struct Capture;
//!
//! impl WindowsCaptureHandler for Capture {
//!     // To Get The Message From The Settings
//!     type Flags = String;
//!
//!     // To Redirect To CaptureControl Or Start Method
//!     type Error = Box<dyn std::error::Error + Send + Sync>;
//!
//!     // Function That Will Be Called To Create The Struct The Flags Can Be Passed
//!     // From `WindowsCaptureSettings`
//!     fn new(message: Self::Flags) -> Result<Self, Self::Error> {
//!         println!("Got The Flag: {message}");
//!
//!         Ok(Self {})
//!     }
//!
//!     // Called Every Time A New Frame Is Available
//!     fn on_frame_arrived(
//!         &mut self,
//!         frame: &mut Frame,
//!         capture_control: InternalCaptureControl,
//!     ) -> Result<(), Self::Error> {
//!         println!("New Frame Arrived");
//!
//!         // Save The Frame As An Image To The Specified Path
//!         frame.save_as_image("image.png")?;
//!
//!         // Gracefully Stop The Capture Thread
//!         capture_control.stop();
//!
//!         Ok(())
//!     }
//!
//!     // Called When The Capture Item Closes Usually When The Window Closes, Capture
//!     // Session Will End After This Function Ends
//!     fn on_closed(&mut self) -> Result<(), Self::Error> {
//!         println!("Capture Session Closed");
//!
//!         Ok(())
//!     }
//! }
//!
//! // Checkout Docs For Other Capture Items
//! let foreground_window = Window::foreground().expect("No Active Window Found");
//!
//! let settings = Settings::new(
//!     // Item To Captue
//!     foreground_window,
//!     // Capture Cursor
//!     Some(true),
//!     // Draw Borders (None Means Default Api Configuration)
//!     None,
//!     // Kind Of Pixel Format For Frame To Have
//!     ColorFormat::Rgba8,
//!     // Will Be Passed To The New Function
//!     "It Works".to_string(),
//! )
//! .unwrap();
//!
//! // Every Error From `new`, `on_frame_arrived` and `on_closed` Will End Up Here
//! Capture::start(settings).unwrap();
//! ```
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]

pub mod capture;
mod d3d11;
pub mod frame;
pub mod graphics_capture_api;
pub mod monitor;
pub mod settings;
pub mod window;
