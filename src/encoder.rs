use std::{
    fs::{self, File},
    path::Path,
    slice,
    sync::{
        atomic::{self, AtomicBool},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
};

use parking_lot::{Condvar, Mutex};
use windows::{
    core::HSTRING,
    Foundation::{EventRegistrationToken, TimeSpan, TypedEventHandler},
    Graphics::{
        DirectX::Direct3D11::IDirect3DSurface,
        Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat},
    },
    Media::{
        Core::{
            MediaStreamSample, MediaStreamSource, MediaStreamSourceSampleRequestedEventArgs,
            MediaStreamSourceStartingEventArgs, VideoStreamDescriptor,
        },
        MediaProperties::{
            MediaEncodingProfile, MediaEncodingSubtypes, VideoEncodingProperties,
            VideoEncodingQuality,
        },
        Transcoding::MediaTranscoder,
    },
    Security::Cryptography::CryptographicBuffer,
    Storage::{
        FileAccessMode, StorageFile,
        Streams::{
            Buffer, DataReader, IRandomAccessStream, InMemoryRandomAccessStream, InputStreamOptions,
        },
    },
};

use crate::{
    d3d11::SendDirectX,
    frame::{Frame, ImageFormat},
    settings::ColorFormat,
};

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
pub enum ImageEncoderError {
    #[error("This color format is not supported for saving as image")]
    UnsupportedFormat,
    #[error("Windows API Error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

/// The `ImageEncoder` struct represents an image encoder that can be used to encode image buffers to image bytes with a specified format and color format.
pub struct ImageEncoder {
    format: ImageFormat,
    color_format: ColorFormat,
}

impl ImageEncoder {
    /// Create a new ImageEncoder with the specified format and color format.
    ///
    /// # Arguments
    ///
    /// * `format` - The desired image format.
    /// * `color_format` - The desired color format.
    ///
    /// # Returns
    ///
    /// A new `ImageEncoder` instance.
    pub const fn new(format: ImageFormat, color_format: ColorFormat) -> Self {
        Self {
            format,
            color_format,
        }
    }

    /// Encode the image buffer to image bytes with the specified format.
    ///
    /// # Arguments
    ///
    /// * `image_buffer` - The image buffer to encode.
    /// * `width` - The width of the image.
    /// * `height` - The height of the image.
    ///
    /// # Returns
    ///
    /// The encoded image bytes as a `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns an `Error` if the encoding fails or if the color format is unsupported.
    pub fn encode(
        &self,
        image_buffer: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, ImageEncoderError> {
        let encoder = match self.format {
            ImageFormat::Jpeg => BitmapEncoder::JpegEncoderId()?,
            ImageFormat::Png => BitmapEncoder::PngEncoderId()?,
            ImageFormat::Gif => BitmapEncoder::GifEncoderId()?,
            ImageFormat::Tiff => BitmapEncoder::TiffEncoderId()?,
            ImageFormat::Bmp => BitmapEncoder::BmpEncoderId()?,
            ImageFormat::JpegXr => BitmapEncoder::JpegXREncoderId()?,
        };

        let stream = InMemoryRandomAccessStream::new()?;
        let encoder = BitmapEncoder::CreateAsync(encoder, &stream)?.get()?;

        let pixelformat = match self.color_format {
            ColorFormat::Bgra8 => BitmapPixelFormat::Bgra8,
            ColorFormat::Rgba8 => BitmapPixelFormat::Rgba8,
            ColorFormat::Rgba16F => return Err(ImageEncoderError::UnsupportedFormat),
        };

        encoder.SetPixelData(
            pixelformat,
            BitmapAlphaMode::Premultiplied,
            width,
            height,
            1.0,
            1.0,
            image_buffer,
        )?;

        encoder.FlushAsync()?.get()?;

        let buffer = Buffer::Create(u32::try_from(stream.Size()?).unwrap())?;
        stream
            .ReadAsync(&buffer, buffer.Capacity()?, InputStreamOptions::None)?
            .get()?;

        let data_reader = DataReader::FromBuffer(&buffer)?;
        let length = data_reader.UnconsumedBufferLength()?;
        let mut bytes = vec![0u8; length as usize];
        data_reader.ReadBytes(&mut bytes)?;

        Ok(bytes)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum VideoEncoderError {
    #[error("Windows API Error: {0}")]
    WindowsError(#[from] windows::core::Error),
    #[error("Frame send error")]
    FrameSendError(#[from] mpsc::SendError<Option<(VideoEncoderSource, TimeSpan)>>),
    #[error("IO Error: {0}")]
    IoError(#[from] std::io::Error),
}

unsafe impl Send for VideoEncoderError {}
unsafe impl Sync for VideoEncoderError {}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum VideoEncoderType {
    Avi,
    Hevc,
    Mp4,
    Wmv,
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum VideoEncoderQuality {
    Auto = 0,
    HD1080p = 1,
    HD720p = 2,
    Wvga = 3,
    Ntsc = 4,
    Pal = 5,
    Vga = 6,
    Qvga = 7,
    Uhd2160p = 8,
    Uhd4320p = 9,
}

/// The `VideoEncoderSource` struct represents all the types that can be send to the encoder.
pub enum VideoEncoderSource {
    DirectX(SendDirectX<IDirect3DSurface>),
    Buffer((SendDirectX<*const u8>, usize)),
}

/// The `VideoEncoder` struct represents a video encoder that can be used to encode video frames and save them to a specified file path.
pub struct VideoEncoder {
    first_timespan: Option<TimeSpan>,
    frame_sender: mpsc::Sender<Option<(VideoEncoderSource, TimeSpan)>>,
    sample_requested: EventRegistrationToken,
    media_stream_source: MediaStreamSource,
    starting: EventRegistrationToken,
    transcode_thread: Option<JoinHandle<Result<(), VideoEncoderError>>>,
    frame_notify: Arc<(Mutex<bool>, Condvar)>,
    error_notify: Arc<AtomicBool>,
}

impl VideoEncoder {
    /// Creates a new `VideoEncoder` instance with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `encoder_type` - The type of video encoder to use.
    /// * `encoder_quality` - The quality of the video encoder.
    /// * `width` - The width of the video frames.
    /// * `height` - The height of the video frames.
    /// * `path` - The file path where the encoded video will be saved.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the `VideoEncoder` instance if successful, or a
    /// `VideoEncoderError` if an error occurs.
    pub fn new<P: AsRef<Path>>(
        encoder_type: VideoEncoderType,
        encoder_quality: VideoEncoderQuality,
        width: u32,
        height: u32,
        path: P,
    ) -> Result<Self, VideoEncoderError> {
        let path = path.as_ref();

        let media_encoding_profile = match encoder_type {
            VideoEncoderType::Avi => {
                MediaEncodingProfile::CreateAvi(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Hevc => {
                MediaEncodingProfile::CreateHevc(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Mp4 => {
                MediaEncodingProfile::CreateMp4(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Wmv => {
                MediaEncodingProfile::CreateWmv(VideoEncodingQuality(encoder_quality as i32))?
            }
        };

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            width,
            height,
        )?;

        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let media_stream_source =
            MediaStreamSource::CreateFromDescriptor(&video_stream_descriptor)?;
        media_stream_source.SetBufferTime(TimeSpan::default())?;

        let (frame_sender, frame_receiver) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();

        let starting = media_stream_source.Starting(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceStartingEventArgs,
        >::new(move |_, stream_start| {
            let stream_start = stream_start
                .as_ref()
                .expect("MediaStreamSource Starting parameter was None This Should Not Happen.");

            stream_start
                .Request()?
                .SetActualStartPosition(TimeSpan { Duration: 0 })?;
            Ok(())
        }))?;

        let frame_notify = Arc::new((Mutex::new(false), Condvar::new()));

        let sample_requested = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new({
            let frame_receiver = frame_receiver;
            let frame_notify = frame_notify.clone();

            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None This Should Not Happen.",
                );

                let frame = match frame_receiver.recv() {
                    Ok(frame) => frame,
                    Err(e) => panic!("Failed to receive frame from frame sender: {e}"),
                };

                match frame {
                    Some((source, timespan)) => {
                        let sample = match source {
                            VideoEncoderSource::DirectX(surface) => {
                                MediaStreamSample::CreateFromDirect3D11Surface(
                                    &surface.0, timespan,
                                )?
                            }
                            VideoEncoderSource::Buffer(buffer_data) => {
                                let buffer = buffer_data.0;
                                let buffer =
                                    unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                MediaStreamSample::CreateFromBuffer(&buffer, timespan)?
                            }
                        };

                        sample_requested.Request()?.SetSample(&sample)?;
                    }
                    None => {
                        sample_requested.Request()?.SetSample(None)?;
                    }
                }

                let (lock, cvar) = &*frame_notify;
                *lock.lock() = true;
                cvar.notify_one();

                Ok(())
            }
        }))?;

        let media_transcoder = MediaTranscoder::new()?;
        media_transcoder.SetHardwareAccelerationEnabled(true)?;

        File::create(path)?;
        let path = fs::canonicalize(path).unwrap().to_string_lossy()[4..].to_string();
        let path = Path::new(&path);

        let path = &HSTRING::from(path.as_os_str().to_os_string());

        let file = StorageFile::GetFileFromPathAsync(path)?.get()?;
        let media_stream_output = file.OpenAsync(FileAccessMode::ReadWrite)?.get()?;

        let transcode = media_transcoder
            .PrepareMediaStreamSourceTranscodeAsync(
                &media_stream_source,
                &media_stream_output,
                &media_encoding_profile,
            )?
            .get()?;

        let error_notify = Arc::new(AtomicBool::new(false));
        let transcode_thread = thread::spawn({
            let error_notify = error_notify.clone();

            move || -> Result<(), VideoEncoderError> {
                let result = transcode.TranscodeAsync();

                if result.is_err() {
                    error_notify.store(true, atomic::Ordering::Relaxed);
                }

                result?.get()?;

                drop(media_transcoder);

                Ok(())
            }
        });

        Ok(Self {
            first_timespan: None,
            frame_sender,
            sample_requested,
            media_stream_source,
            starting,
            transcode_thread: Some(transcode_thread),
            frame_notify,
            error_notify,
        })
    }

    /// Creates a new `VideoEncoder` instance with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `encoder_type` - The type of video encoder to use.
    /// * `encoder_quality` - The quality of the video encoder.
    /// * `width` - The width of the video frames.
    /// * `height` - The height of the video frames.
    /// * `stream` - The stream where the encoded video will be saved.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the `VideoEncoder` instance if successful, or a
    /// `VideoEncoderError` if an error occurs.
    pub fn new_from_stream<P: AsRef<Path>>(
        encoder_type: VideoEncoderType,
        encoder_quality: VideoEncoderQuality,
        width: u32,
        height: u32,
        stream: IRandomAccessStream,
    ) -> Result<Self, VideoEncoderError> {
        let media_encoding_profile = match encoder_type {
            VideoEncoderType::Avi => {
                MediaEncodingProfile::CreateAvi(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Hevc => {
                MediaEncodingProfile::CreateHevc(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Mp4 => {
                MediaEncodingProfile::CreateMp4(VideoEncodingQuality(encoder_quality as i32))?
            }
            VideoEncoderType::Wmv => {
                MediaEncodingProfile::CreateWmv(VideoEncodingQuality(encoder_quality as i32))?
            }
        };

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            width,
            height,
        )?;

        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let media_stream_source =
            MediaStreamSource::CreateFromDescriptor(&video_stream_descriptor)?;
        media_stream_source.SetBufferTime(TimeSpan::default())?;

        let (frame_sender, frame_receiver) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();

        let starting = media_stream_source.Starting(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceStartingEventArgs,
        >::new(move |_, stream_start| {
            let stream_start = stream_start
                .as_ref()
                .expect("MediaStreamSource Starting parameter was None This Should Not Happen.");

            stream_start
                .Request()?
                .SetActualStartPosition(TimeSpan { Duration: 0 })?;
            Ok(())
        }))?;

        let frame_notify = Arc::new((Mutex::new(false), Condvar::new()));

        let sample_requested = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new({
            let frame_receiver = frame_receiver;
            let frame_notify = frame_notify.clone();

            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None This Should Not Happen.",
                );

                let frame = match frame_receiver.recv() {
                    Ok(frame) => frame,
                    Err(e) => panic!("Failed to receive frame from frame sender: {e}"),
                };

                match frame {
                    Some((source, timespan)) => {
                        let sample = match source {
                            VideoEncoderSource::DirectX(surface) => {
                                MediaStreamSample::CreateFromDirect3D11Surface(
                                    &surface.0, timespan,
                                )?
                            }
                            VideoEncoderSource::Buffer(buffer_data) => {
                                let buffer = buffer_data.0;
                                let buffer =
                                    unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                MediaStreamSample::CreateFromBuffer(&buffer, timespan)?
                            }
                        };

                        sample_requested.Request()?.SetSample(&sample)?;
                    }
                    None => {
                        sample_requested.Request()?.SetSample(None)?;
                    }
                }

                let (lock, cvar) = &*frame_notify;
                *lock.lock() = true;
                cvar.notify_one();

                Ok(())
            }
        }))?;

        let media_transcoder = MediaTranscoder::new()?;
        media_transcoder.SetHardwareAccelerationEnabled(true)?;

        let transcode = media_transcoder
            .PrepareMediaStreamSourceTranscodeAsync(
                &media_stream_source,
                &stream,
                &media_encoding_profile,
            )?
            .get()?;

        let error_notify = Arc::new(AtomicBool::new(false));
        let transcode_thread = thread::spawn({
            let error_notify = error_notify.clone();

            move || -> Result<(), VideoEncoderError> {
                let result = transcode.TranscodeAsync();

                if result.is_err() {
                    error_notify.store(true, atomic::Ordering::Relaxed);
                }

                result?.get()?;

                drop(media_transcoder);

                Ok(())
            }
        });

        Ok(Self {
            first_timespan: None,
            frame_sender,
            sample_requested,
            media_stream_source,
            starting,
            transcode_thread: Some(transcode_thread),
            frame_notify,
            error_notify,
        })
    }

    /// Sends a video frame to the video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `frame` - A mutable reference to the `Frame` to be encoded.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    pub fn send_frame(&mut self, frame: &mut Frame) -> Result<(), VideoEncoderError> {
        let timespan = match self.first_timespan {
            Some(timespan) => TimeSpan {
                Duration: frame.timespan().Duration - timespan.Duration,
            },
            None => {
                let timespan = frame.timespan();
                self.first_timespan = Some(timespan);
                TimeSpan { Duration: 0 }
            }
        };
        let surface = SendDirectX::new(unsafe { frame.as_raw_surface() });

        self.frame_sender
            .send(Some((VideoEncoderSource::DirectX(surface), timespan)))?;

        let (lock, cvar) = &*self.frame_notify;
        let mut processed = lock.lock();
        if !*processed {
            cvar.wait(&mut processed);
        }
        *processed = false;
        drop(processed);

        if self.error_notify.load(atomic::Ordering::Relaxed) {
            if let Some(transcode_thread) = self.transcode_thread.take() {
                transcode_thread
                    .join()
                    .expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Sends a video frame to the video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `buffer` - A reference to the byte slice to be encoded Windows API expect this to be Bgra and bottom-top.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    pub fn send_frame_buffer(
        &mut self,
        buffer: &[u8],
        timespan: i64,
    ) -> Result<(), VideoEncoderError> {
        let timespan = TimeSpan { Duration: timespan };

        self.frame_sender.send(Some((
            VideoEncoderSource::Buffer((SendDirectX::new(buffer.as_ptr()), buffer.len())),
            timespan,
        )))?;

        let (lock, cvar) = &*self.frame_notify;
        let mut processed = lock.lock();
        if !*processed {
            cvar.wait(&mut processed);
        }
        *processed = false;
        drop(processed);

        if self.error_notify.load(atomic::Ordering::Relaxed) {
            if let Some(transcode_thread) = self.transcode_thread.take() {
                transcode_thread
                    .join()
                    .expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Finishes encoding the video and performs any necessary cleanup.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the encoding is successfully finished, or a `VideoEncoderError` if an
    /// error occurs.
    pub fn finish(mut self) -> Result<(), VideoEncoderError> {
        self.frame_sender.send(None)?;

        if let Some(transcode_thread) = self.transcode_thread.take() {
            transcode_thread
                .join()
                .expect("Failed to join transcode thread")?;
        }

        self.media_stream_source.RemoveStarting(self.starting)?;
        self.media_stream_source
            .RemoveSampleRequested(self.sample_requested)?;

        Ok(())
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        let _ = self.frame_sender.send(None);

        if let Some(transcode_thread) = self.transcode_thread.take() {
            let _ = transcode_thread.join();
        }
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for VideoEncoder {}
