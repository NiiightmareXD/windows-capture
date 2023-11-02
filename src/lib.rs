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
//! windows-capture = "1.0.22"
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
//!     capture::WindowsCaptureHandler, frame::Frame, settings::WindowsCaptureSettings,
//!     window::Window,
//! };
//!
//! struct Capture;
//!
//! impl WindowsCaptureHandler for Capture {
//!     type Flags = String; // To Get The Message (Or A Variable Or ...) From The Settings
//!
//!     fn new(message: Self::Flags) -> Self {
//!         // Function That Will Be Called To Create The Struct The Flags Can Be Passed
//!         // From Settings
//!         println!("Got The Message: {message}");
//!
//!         Self {}
//!     }
//!
//!     fn on_frame_arrived(&mut self, mut frame: Frame) {
//!         // Called Every Time A New Frame Is Available
//!         println!("Got A New Frame");
//!
//!         // Save The Frame As An Image To Specified Path
//!         frame.save_as_image("image.png").unwrap();
//!
//!         // Call To Stop The Capture Thread, You Might Receive A Few More Frames
//!         // Before It Stops
//!         self.stop();
//!     }
//!
//!     fn on_closed(&mut self) {
//!         // Called When The Capture Item Closes Usually When The Window Closes,
//!         // Capture Will End After This Function Ends
//!         println!("Capture Item Closed");
//!     }
//! }
//!
//! // Checkout Docs For Other Capture Items
//! let foreground_window = Window::foreground().unwrap();
//!
//! let settings = WindowsCaptureSettings::new(
//!     // Item To Captue
//!     foreground_window,
//!     // Capture Cursor
//!     Some(true),
//!     // Draw Border
//!     Some(false),
//!     // This Will Be Passed To The New Function
//!     "It Works".to_string(),
//! )
//! .unwrap();
//!
//! Capture::start(settings).unwrap();
//! ```
#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::inconsistent_struct_constructor)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::ptr_as_ptr)]
#![warn(clippy::borrow_as_ptr)]
#![warn(clippy::nursery)]
#![warn(clippy::cargo)]

mod buffer;
pub mod capture;
mod d3d11;
pub mod frame;
pub mod graphics_capture_api;
pub mod monitor;
pub mod settings;
pub mod window;
