use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::io::Write;

use crate::encoder::{EncodedFrame, EncodedAudioFrame, FrameCallback};

/// Network transmission protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// TCP protocol for reliable transmission
    Tcp,
    /// UDP protocol for fast transmission with potential packet loss
    Udp,
    /// WebRTC protocol for real-time communication
    WebRtc,
    /// RTMP protocol for streaming
    Rtmp,
}

/// Configuration for network transmission
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// The protocol to use for transmission
    pub protocol: Protocol,
    /// The target address (e.g., "127.0.0.1:8080")
    pub address: String,
    /// Maximum frame buffer size
    pub max_buffer_size: usize,
    /// Frame rate for transmission
    pub frame_rate: u32,
    /// Quality settings
    pub quality: Quality,
}

/// Quality settings for network transmission
#[derive(Debug, Clone)]
pub struct Quality {
    /// Video bitrate in bits per second
    pub video_bitrate: u32,
    /// Audio bitrate in bits per second
    pub audio_bitrate: u32,
    /// Maximum frame size in bytes
    pub max_frame_size: usize,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            video_bitrate: 2_000_000, // 2 Mbps
            audio_bitrate: 128_000,   // 128 kbps
            max_frame_size: 1024 * 1024, // 1 MB
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::Tcp,
            address: "127.0.0.1:8080".to_string(),
            max_buffer_size: 100,
            frame_rate: 30,
            quality: Quality::default(),
        }
    }
}

/// A simple TCP server for streaming encoded frames
pub struct TcpStreamServer {
    listener: TcpListener,
    clients: Arc<Mutex<Vec<TcpStream>>>,
    frame_buffer: Arc<Mutex<VecDeque<EncodedFrame>>>,
    audio_buffer: Arc<Mutex<VecDeque<EncodedAudioFrame>>>,
    config: NetworkConfig,
}

impl TcpStreamServer {
    /// Creates a new TCP streaming server
    ///
    /// # Arguments
    ///
    /// * `config` - The network configuration
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the server instance if successful
    pub fn new(config: NetworkConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&config.address)?;
        println!("TCP streaming server started on {}", config.address);

        Ok(Self {
            listener,
            clients: Arc::new(Mutex::new(Vec::new())),
            frame_buffer: Arc::new(Mutex::new(VecDeque::new())),
            audio_buffer: Arc::new(Mutex::new(VecDeque::new())),
            config,
        })
    }

    /// Starts the server and begins accepting connections
    pub fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let clients = self.clients.clone();
        let listener = self.listener.try_clone()?;

        // Accept connections in a separate thread
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        println!("New client connected");
                        if let Ok(mut clients) = clients.lock() {
                            clients.push(stream);
                        }
                    }
                    Err(e) => eprintln!("Failed to accept connection: {}", e),
                }
            }
        });

        Ok(())
    }

    /// Broadcasts a frame to all connected clients
    pub fn broadcast_frame(&self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut clients = self.clients.lock().unwrap();
        let frame_data = self.serialize_frame(&frame)?;

        // Remove disconnected clients
    clients.retain(|mut client| {
            if let Err(_) = client.write_all(&frame_data) {
                false
            } else {
                true
            }
        });

        Ok(())
    }

    /// Serializes a frame for network transmission
    fn serialize_frame(&self, frame: &EncodedFrame) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        use std::io::{Cursor, Write};
        use byteorder::{LittleEndian, WriteBytesExt};

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);

        // Write frame header
        cursor.write_u32::<LittleEndian>(frame.data.len() as u32)?;
        cursor.write_i64::<LittleEndian>(frame.timestamp)?;
        cursor.write_u32::<LittleEndian>(frame.frame_type as u32)?;
        cursor.write_u32::<LittleEndian>(frame.width)?;
        cursor.write_u32::<LittleEndian>(frame.height)?;

        // Write frame data
        cursor.write_all(&frame.data)?;

        Ok(buffer)
    }
}

/// A UDP streaming client for sending encoded frames
pub struct UdpStreamClient {
    socket: UdpSocket,
    config: NetworkConfig,
    frame_count: u64,
    last_frame_time: Instant,
}

impl UdpStreamClient {
    /// Creates a new UDP streaming client
    ///
    /// # Arguments
    ///
    /// * `config` - The network configuration
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the client instance if successful
    pub fn new(config: NetworkConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.connect(&config.address)?;
        println!("UDP streaming client connected to {}", config.address);

        Ok(Self {
            socket,
            config,
            frame_count: 0,
            last_frame_time: Instant::now(),
        })
    }

    /// Sends a frame over UDP
    pub fn send_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Rate limiting
        let now = Instant::now();
        let frame_interval = Duration::from_secs_f64(1.0 / self.config.frame_rate as f64);
        
        if now.duration_since(self.last_frame_time) < frame_interval {
            return Ok(());
        }

        let frame_data = self.serialize_frame(&frame)?;
        
        if frame_data.len() > self.config.quality.max_frame_size {
            eprintln!("Frame too large: {} bytes", frame_data.len());
            return Ok(());
        }

        self.socket.send(&frame_data)?;
        self.frame_count += 1;
        self.last_frame_time = now;

        Ok(())
    }

    /// Serializes a frame for UDP transmission
    fn serialize_frame(&self, frame: &EncodedFrame) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        use std::io::{Cursor, Write};
        use byteorder::{LittleEndian, WriteBytesExt};

        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);

        // Write frame header
        cursor.write_u32::<LittleEndian>(frame.data.len() as u32)?;
        cursor.write_i64::<LittleEndian>(frame.timestamp)?;
        cursor.write_u32::<LittleEndian>(frame.frame_type as u32)?;
        cursor.write_u32::<LittleEndian>(frame.width)?;
        cursor.write_u32::<LittleEndian>(frame.height)?;

        // Write frame data
        cursor.write_all(&frame.data)?;

        Ok(buffer)
    }
}

/// A network callback that implements FrameCallback for streaming
pub struct NetworkCallback {
    tcp_server: Option<TcpStreamServer>,
    udp_client: Option<UdpStreamClient>,
    config: NetworkConfig,
}

impl NetworkCallback {
    /// Creates a new network callback
    ///
    /// # Arguments
    ///
    /// * `config` - The network configuration
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the callback instance if successful
    pub fn new(config: NetworkConfig) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let tcp_server = if config.protocol == Protocol::Tcp {
            Some(TcpStreamServer::new(config.clone())?)
        } else {
            None
        };

        let udp_client = if config.protocol == Protocol::Udp {
            Some(UdpStreamClient::new(config.clone())?)
        } else {
            None
        };

        Ok(Self {
            tcp_server,
            udp_client,
            config,
        })
    }

    /// Starts the network services
    pub fn start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(ref server) = self.tcp_server {
            server.start()?;
        }
        Ok(())
    }
}

impl FrameCallback for NetworkCallback {
    fn on_video_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match self.config.protocol {
            Protocol::Tcp => {
                if let Some(ref server) = self.tcp_server {
                    server.broadcast_frame(frame)?;
                }
            }
            Protocol::Udp => {
                if let Some(ref mut client) = self.udp_client {
                    client.send_frame(frame)?;
                }
            }
            Protocol::WebRtc => {
                // WebRTC implementation would go here
                eprintln!("WebRTC protocol not yet implemented");
            }
            Protocol::Rtmp => {
                // RTMP implementation would go here
                eprintln!("RTMP protocol not yet implemented");
            }
        }
        Ok(())
    }

    fn on_audio_frame(&mut self, frame: EncodedAudioFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Audio frame handling would go here
        // For now, we'll just log it
        println!("Received audio frame: {} bytes, timestamp: {}", frame.data.len(), frame.timestamp);
        Ok(())
    }

    fn on_stream_start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Network stream started");
        self.start()?;
        Ok(())
    }

    fn on_stream_end(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("Network stream ended");
        Ok(())
    }
}

/// A simple file callback for debugging and testing
pub struct FileCallback {
    output_dir: String,
    frame_count: u64,
}

impl FileCallback {
    /// Creates a new file callback for saving encoded frames
    ///
    /// # Arguments
    ///
    /// * `output_dir` - The directory to save frames to
    ///
    /// # Returns
    ///
    /// Returns a new FileCallback instance
    pub fn new(output_dir: String) -> Self {
        std::fs::create_dir_all(&output_dir).unwrap_or_else(|_| {
            eprintln!("Failed to create output directory: {}", output_dir);
        });

        Self {
            output_dir,
            frame_count: 0,
        }
    }
}

impl FrameCallback for FileCallback {
    fn on_video_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let filename = format!("{}/frame_{:06}.h264", self.output_dir, self.frame_count);
        std::fs::write(&filename, &frame.data)?;
        self.frame_count += 1;
        println!("Saved frame {} to {}", self.frame_count, filename);
        Ok(())
    }

    fn on_audio_frame(&mut self, frame: EncodedAudioFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let filename = format!("{}/audio_{:06}.aac", self.output_dir, self.frame_count);
        std::fs::write(&filename, &frame.data)?;
        println!("Saved audio frame to {}", filename);
        Ok(())
    }

    fn on_stream_start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("File stream started, saving to: {}", self.output_dir);
        Ok(())
    }

    fn on_stream_end(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        println!("File stream ended, saved {} frames", self.frame_count);
        Ok(())
    }
}