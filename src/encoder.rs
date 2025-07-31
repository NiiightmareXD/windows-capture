use std::fs::{self, File};
use std::path::Path;
use std::slice;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use parking_lot::{Condvar, Mutex};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Graphics::DirectX::Direct3D11::IDirect3DSurface;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapEncoder, BitmapPixelFormat};
use windows::Media::Core::{
    AudioStreamDescriptor, MediaStreamSample, MediaStreamSource,
    MediaStreamSourceSampleRequestedEventArgs, MediaStreamSourceStartingEventArgs,
    VideoStreamDescriptor,
};
use windows::Media::MediaProperties::{
    AudioEncodingProperties, ContainerEncodingProperties, MediaEncodingProfile,
    MediaEncodingSubtypes, VideoEncodingProperties,
};
use windows::Media::Transcoding::MediaTranscoder;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Storage::Streams::{
    Buffer, DataReader, IRandomAccessStream, InMemoryRandomAccessStream, InputStreamOptions,
};
use windows::Storage::{FileAccessMode, StorageFile};
use windows::core::{HSTRING, Interface};

use crate::d3d11::SendDirectX;
use crate::frame::{Frame, ImageFormat};
use crate::settings::ColorFormat;

/// Represents an encoded video frame with metadata
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// The encoded frame data
    pub data: Vec<u8>,
    /// The timestamp of the frame in 100-nanosecond units
    pub timestamp: i64,
    /// The frame type (keyframe, delta frame, etc.)
    pub frame_type: FrameType,
    /// The width of the original frame
    pub width: u32,
    /// The height of the original frame
    pub height: u32,
}

/// Represents the type of encoded frame
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    /// Key frame (I-frame) - contains complete frame data
    KeyFrame,
    /// Delta frame (P-frame) - contains only changes from previous frame
    DeltaFrame,
    /// Bidirectional frame (B-frame) - depends on both past and future frames
    BidirectionalFrame,
}

/// Represents an encoded audio frame with metadata
#[derive(Debug, Clone)]
pub struct EncodedAudioFrame {
    /// The encoded audio data
    pub data: Vec<u8>,
    /// The timestamp of the audio frame in 100-nanosecond units
    pub timestamp: i64,
    /// The number of audio samples in this frame
    pub sample_count: u32,
}

/// Callback trait for handling encoded frames in real-time
pub trait FrameCallback: Send + Sync {
    /// Called when a new encoded video frame is available
    fn on_video_frame(&mut self, frame: EncodedFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Called when a new encoded audio frame is available
    fn on_audio_frame(&mut self, frame: EncodedAudioFrame) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Called when the stream starts
    fn on_stream_start(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
    
    /// Called when the stream ends
    fn on_stream_end(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
pub enum ImageEncoderError {
    #[error("This color format is not supported for saving as an image")]
    UnsupportedFormat,
    #[error("Windows API error: {0}")]
    WindowsError(#[from] windows::core::Error),
}

/// The `ImageEncoder` struct is used to encode image buffers into image bytes with a specified format and color format.
pub struct ImageEncoder {
    format: ImageFormat,
    color_format: ColorFormat,
}

impl ImageEncoder {
    /// Creates a new `ImageEncoder` with the specified format and color format.
    ///
    /// # Arguments
    ///
    /// * `format` - The desired image format.
    /// * `color_format` - The desired color format.
    ///
    /// # Returns
    ///
    /// A new `ImageEncoder` instance.
    #[must_use]
    #[inline]
    pub const fn new(format: ImageFormat, color_format: ColorFormat) -> Self {
        Self { format, color_format }
    }

    /// Encodes the image buffer into image bytes with the specified format.
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
    /// Returns an `ImageEncoderError` if the encoding fails or if the color format is unsupported.
    #[inline]
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
        stream.ReadAsync(&buffer, buffer.Capacity()?, InputStreamOptions::None)?.get()?;

        let data_reader = DataReader::FromBuffer(&buffer)?;
        let length = data_reader.UnconsumedBufferLength()?;
        let mut bytes = vec![0u8; length as usize];
        data_reader.ReadBytes(&mut bytes)?;

        Ok(bytes)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum VideoEncoderError {
    #[error("Windows API error: {0}")]
    WindowsError(#[from] windows::core::Error),
    #[error("Failed to send frame: {0}")]
    FrameSendError(#[from] mpsc::SendError<Option<(VideoEncoderSource, TimeSpan)>>),
    #[error("Failed to send audio: {0}")]
    AudioSendError(#[from] mpsc::SendError<Option<(AudioEncoderSource, TimeSpan)>>),
    #[error("Video encoding is disabled")]
    VideoDisabled,
    #[error("Audio encoding is disabled")]
    AudioDisabled,
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

unsafe impl Send for VideoEncoderError {}
unsafe impl Sync for VideoEncoderError {}

/// The `VideoEncoderSource` enum represents all the types that can be sent to the encoder.
pub enum VideoEncoderSource {
    DirectX(SendDirectX<IDirect3DSurface>),
    Buffer((SendDirectX<*const u8>, usize)),
}

/// The `AudioEncoderSource` enum represents all the types that can be sent to the encoder.
pub enum AudioEncoderSource {
    Buffer((SendDirectX<*const u8>, usize)),
}

/// The `VideoSettingsBuilder` struct is used to configure settings for the video encoder.
pub struct VideoSettingsBuilder {
    sub_type: VideoSettingsSubType,
    bitrate: u32,
    width: u32,
    height: u32,
    frame_rate: u32,
    pixel_aspect_ratio: (u32, u32),
    disabled: bool,
}

impl VideoSettingsBuilder {
    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            bitrate: 15000000,
            frame_rate: 60,
            pixel_aspect_ratio: (1, 1),
            sub_type: VideoSettingsSubType::HEVC,
            width,
            height,
            disabled: false,
        }
    }

    pub const fn sub_type(mut self, sub_type: VideoSettingsSubType) -> Self {
        self.sub_type = sub_type;
        self
    }

    pub const fn bitrate(mut self, bitrate: u32) -> Self {
        self.bitrate = bitrate;
        self
    }

    pub const fn width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    pub const fn height(mut self, height: u32) -> Self {
        self.height = height;
        self
    }

    pub const fn frame_rate(mut self, frame_rate: u32) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    pub const fn pixel_aspect_ratio(mut self, pixel_aspect_ratio: (u32, u32)) -> Self {
        self.pixel_aspect_ratio = pixel_aspect_ratio;
        self
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn build(self) -> Result<(VideoEncodingProperties, bool), VideoEncoderError> {
        let properties = VideoEncodingProperties::new()?;

        properties.SetSubtype(&self.sub_type.to_hstring())?;
        properties.SetBitrate(self.bitrate)?;
        properties.SetWidth(self.width)?;
        properties.SetHeight(self.height)?;
        properties.FrameRate()?.SetNumerator(self.frame_rate)?;
        properties.FrameRate()?.SetDenominator(1)?;
        properties.PixelAspectRatio()?.SetNumerator(self.pixel_aspect_ratio.0)?;
        properties.PixelAspectRatio()?.SetDenominator(self.pixel_aspect_ratio.1)?;

        Ok((properties, self.disabled))
    }
}

/// The `AudioSettingsBuilder` is used to configure settings for the audio encoder.
pub struct AudioSettingsBuilder {
    bitrate: u32,
    channel_count: u32,
    sample_rate: u32,
    bit_per_sample: u32,
    sub_type: AudioSettingsSubType,
    disabled: bool,
}

impl AudioSettingsBuilder {
    pub const fn new() -> Self {
        Self {
            bitrate: 192000,
            channel_count: 2,
            sample_rate: 48000,
            bit_per_sample: 16,
            sub_type: AudioSettingsSubType::AAC,
            disabled: false,
        }
    }
    pub const fn bitrate(mut self, bitrate: u32) -> Self {
        self.bitrate = bitrate;
        self
    }

    pub const fn channel_count(mut self, channel_count: u32) -> Self {
        self.channel_count = channel_count;
        self
    }

    pub const fn sample_rate(mut self, sample_rate: u32) -> Self {
        self.sample_rate = sample_rate;
        self
    }

    pub const fn bit_per_sample(mut self, bit_per_sample: u32) -> Self {
        self.bit_per_sample = bit_per_sample;
        self
    }

    pub const fn sub_type(mut self, sub_type: AudioSettingsSubType) -> Self {
        self.sub_type = sub_type;
        self
    }

    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn build(self) -> Result<(AudioEncodingProperties, bool), VideoEncoderError> {
        let properties = AudioEncodingProperties::new()?;
        properties.SetBitrate(self.bitrate)?;
        properties.SetChannelCount(self.channel_count)?;
        properties.SetSampleRate(self.sample_rate)?;
        properties.SetBitsPerSample(self.bit_per_sample)?;
        properties.SetSubtype(&self.sub_type.to_hstring())?;

        Ok((properties, self.disabled))
    }
}

impl Default for AudioSettingsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// The `ContainerSettingsBuilder` is used to configure settings for the container.
pub struct ContainerSettingsBuilder {
    sub_type: ContainerSettingsSubType,
}

impl ContainerSettingsBuilder {
    pub const fn new() -> Self {
        Self { sub_type: ContainerSettingsSubType::MPEG4 }
    }

    pub const fn sub_type(mut self, sub_type: ContainerSettingsSubType) -> Self {
        self.sub_type = sub_type;
        self
    }

    fn build(self) -> Result<ContainerEncodingProperties, VideoEncoderError> {
        let properties = ContainerEncodingProperties::new()?;
        properties.SetSubtype(&self.sub_type.to_hstring())?;
        Ok(properties)
    }
}

impl Default for ContainerSettingsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// The `VideoSettingsSubType` enum represents the subtypes for the video encoder.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum VideoSettingsSubType {
    ARGB32,
    BGRA8,
    D16,
    H263,
    H264,
    H264ES,
    HEVC,
    HEVCES,
    IYUV,
    L8,
    L16,
    MJPG,
    NV12,
    MPEG1,
    MPEG2,
    RGB24,
    RGB32,
    WMV3,
    WVC1,
    VP9,
    YUY2,
    YV12,
}

impl VideoSettingsSubType {
    pub fn to_hstring(&self) -> HSTRING {
        let s = match self {
            Self::ARGB32 => "ARGB32",
            Self::BGRA8 => "BGRA8",
            Self::D16 => "D16",
            Self::H263 => "H263",
            Self::H264 => "H264",
            Self::H264ES => "H264ES",
            Self::HEVC => "HEVC",
            Self::HEVCES => "HEVCES",
            Self::IYUV => "IYUV",
            Self::L8 => "L8",
            Self::L16 => "L16",
            Self::MJPG => "MJPG",
            Self::NV12 => "NV12",
            Self::MPEG1 => "MPEG1",
            Self::MPEG2 => "MPEG2",
            Self::RGB24 => "RGB24",
            Self::RGB32 => "RGB32",
            Self::WMV3 => "WMV3",
            Self::WVC1 => "WVC1",
            Self::VP9 => "VP9",
            Self::YUY2 => "YUY2",
            Self::YV12 => "YV12",
        };

        HSTRING::from(s)
    }
}

/// The `AudioSettingsSubType` enum represents the subtypes for the audio encoder.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum AudioSettingsSubType {
    AAC,
    AC3,
    AACADTS,
    AACHDCP,
    AC3SPDIF,
    AC3HDCP,
    ADTS,
    ALAC,
    AMRNB,
    AWRWB,
    DTS,
    EAC3,
    FLAC,
    Float,
    MP3,
    MPEG,
    OPUS,
    PCM,
    WMA8,
    WMA9,
    Vorbis,
}

impl AudioSettingsSubType {
    pub fn to_hstring(&self) -> HSTRING {
        let s = match self {
            Self::AAC => "AAC",
            Self::AC3 => "AC3",
            Self::AACADTS => "AACADTS",
            Self::AACHDCP => "AACHDCP",
            Self::AC3SPDIF => "AC3SPDIF",
            Self::AC3HDCP => "AC3HDCP",
            Self::ADTS => "ADTS",
            Self::ALAC => "ALAC",
            Self::AMRNB => "AMRNB",
            Self::AWRWB => "AWRWB",
            Self::DTS => "DTS",
            Self::EAC3 => "EAC3",
            Self::FLAC => "FLAC",
            Self::Float => "Float",
            Self::MP3 => "MP3",
            Self::MPEG => "MPEG",
            Self::OPUS => "OPUS",
            Self::PCM => "PCM",
            Self::WMA8 => "WMA8",
            Self::WMA9 => "WMA9",
            Self::Vorbis => "Vorbis",
        };

        HSTRING::from(s)
    }
}

/// The `ContainerSettingsSubType` enum represents the subtypes for the container.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum ContainerSettingsSubType {
    ASF,
    MP3,
    MPEG4,
    AVI,
    MPEG2,
    WAVE,
    AACADTS,
    ADTS,
    GP3,
    AMR,
    FLAC,
}

impl ContainerSettingsSubType {
    pub fn to_hstring(&self) -> HSTRING {
        match self {
            Self::ASF => HSTRING::from("ASF"),
            Self::MP3 => HSTRING::from("MP3"),
            Self::MPEG4 => HSTRING::from("MPEG4"),
            Self::AVI => HSTRING::from("AVI"),
            Self::MPEG2 => HSTRING::from("MPEG2"),
            Self::WAVE => HSTRING::from("WAVE"),
            Self::AACADTS => HSTRING::from("AACADTS"),
            Self::ADTS => HSTRING::from("ADTS"),
            Self::GP3 => HSTRING::from("3GP"),
            Self::AMR => HSTRING::from("AMR"),
            Self::FLAC => HSTRING::from("FLAC"),
        }
    }
}

/// The `VideoEncoder` struct is used to encode video frames and save them to a specified file path.
pub struct VideoEncoder {
    first_timestamp: Option<TimeSpan>,
    frame_sender: mpsc::Sender<Option<(VideoEncoderSource, TimeSpan)>>,
    audio_sender: mpsc::Sender<Option<(AudioEncoderSource, TimeSpan)>>,
    sample_requested: i64,
    media_stream_source: MediaStreamSource,
    starting: i64,
    transcode_thread: Option<JoinHandle<Result<(), VideoEncoderError>>>,
    frame_notify: Arc<(Mutex<bool>, Condvar)>,
    audio_notify: Arc<(Mutex<bool>, Condvar)>,
    error_notify: Arc<AtomicBool>,
    is_video_disabled: bool,
    is_audio_disabled: bool,
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
    #[inline]
    pub fn new<P: AsRef<Path>>(
        video_settings: VideoSettingsBuilder,
        audio_settings: AudioSettingsBuilder,
        container_settings: ContainerSettingsBuilder,
        path: P,
    ) -> Result<Self, VideoEncoderError> {
        let path = path.as_ref();
        let media_encoding_profile = MediaEncodingProfile::new()?;

        let (video_encoding_properties, is_video_disabled) = video_settings.build()?;
        media_encoding_profile.SetVideo(&video_encoding_properties)?;
        let (audio_encoding_properties, is_audio_disabled) = audio_settings.build()?;
        media_encoding_profile.SetAudio(&audio_encoding_properties)?;
        let container_encoding_properties = container_settings.build()?;
        media_encoding_profile.SetContainer(&container_encoding_properties)?;

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            video_encoding_properties.Width()?,
            video_encoding_properties.Height()?,
        )?;
        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let audio_encoding_properties = AudioEncodingProperties::CreateAac(
            audio_encoding_properties.SampleRate()?,
            audio_encoding_properties.ChannelCount()?,
            audio_encoding_properties.Bitrate()?,
        )?;
        let audio_stream_descriptor = AudioStreamDescriptor::Create(&audio_encoding_properties)?;

        let media_stream_source = MediaStreamSource::CreateFromDescriptors(
            &video_stream_descriptor,
            &audio_stream_descriptor,
        )?;
        media_stream_source.SetBufferTime(TimeSpan::default())?;

        let starting = media_stream_source.Starting(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceStartingEventArgs,
        >::new(move |_, stream_start| {
            let stream_start = stream_start
                .as_ref()
                .expect("MediaStreamSource Starting parameter was None. This should not happen.");

            stream_start.Request()?.SetActualStartPosition(TimeSpan { Duration: 0 })?;
            Ok(())
        }))?;

        let (frame_sender, frame_receiver) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();

        let (audio_sender, audio_receiver) =
            mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let frame_notify = Arc::new((Mutex::new(false), Condvar::new()));
        let audio_notify = Arc::new((Mutex::new(false), Condvar::new()));

        let sample_requested = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new({
            let frame_receiver = frame_receiver;
            let frame_notify = frame_notify.clone();

            let audio_receiver = audio_receiver;
            let audio_notify = audio_notify.clone();

            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None. This should not happen.",
                );

                if sample_requested
                    .Request()?
                    .StreamDescriptor()?
                    .cast::<AudioStreamDescriptor>()
                    .is_ok()
                {
                    if is_audio_disabled {
                        sample_requested.Request()?.SetSample(None)?;

                        return Ok(());
                    }

                    let audio = match audio_receiver.recv() {
                        Ok(audio) => audio,
                        Err(e) => panic!("Failed to receive audio from the audio sender: {e}"),
                    };

                    match audio {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                AudioEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
                                }
                            };

                            sample_requested.Request()?.SetSample(&sample)?;
                        }
                        None => {
                            sample_requested.Request()?.SetSample(None)?;
                        }
                    }

                    let (lock, cvar) = &*audio_notify;
                    *lock.lock() = true;
                    cvar.notify_one();
                } else {
                    if is_video_disabled {
                        sample_requested.Request()?.SetSample(None)?;

                        return Ok(());
                    }

                    let frame = match frame_receiver.recv() {
                        Ok(frame) => frame,
                        Err(e) => panic!("Failed to receive a frame from the frame sender: {e}"),
                    };

                    match frame {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                VideoEncoderSource::DirectX(surface) => {
                                    MediaStreamSample::CreateFromDirect3D11Surface(
                                        &surface.0, timestamp,
                                    )?
                                }
                                VideoEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
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
                }

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
            first_timestamp: None,
            frame_sender,
            audio_sender,
            sample_requested,
            media_stream_source,
            starting,
            transcode_thread: Some(transcode_thread),
            frame_notify,
            audio_notify,
            error_notify,
            is_video_disabled,
            is_audio_disabled,
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
    #[inline]
    pub fn new_from_stream(
        video_settings: VideoSettingsBuilder,
        audio_settings: AudioSettingsBuilder,
        container_settings: ContainerSettingsBuilder,
        stream: IRandomAccessStream,
    ) -> Result<Self, VideoEncoderError> {
        let media_encoding_profile = MediaEncodingProfile::new()?;

        let (video_encoding_properties, is_video_disabled) = video_settings.build()?;
        media_encoding_profile.SetVideo(&video_encoding_properties)?;
        let (audio_encoding_properties, is_audio_disabled) = audio_settings.build()?;
        media_encoding_profile.SetAudio(&audio_encoding_properties)?;
        let container_encoding_properties = container_settings.build()?;
        media_encoding_profile.SetContainer(&container_encoding_properties)?;

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            video_encoding_properties.Width()?,
            video_encoding_properties.Height()?,
        )?;
        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let audio_encoding_properties = AudioEncodingProperties::CreateAac(
            audio_encoding_properties.SampleRate()?,
            audio_encoding_properties.ChannelCount()?,
            audio_encoding_properties.Bitrate()?,
        )?;
        let audio_stream_descriptor = AudioStreamDescriptor::Create(&audio_encoding_properties)?;

        let media_stream_source = MediaStreamSource::CreateFromDescriptors(
            &video_stream_descriptor,
            &audio_stream_descriptor,
        )?;
        media_stream_source.SetBufferTime(TimeSpan::default())?;

        let starting = media_stream_source.Starting(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceStartingEventArgs,
        >::new(move |_, stream_start| {
            let stream_start = stream_start
                .as_ref()
                .expect("MediaStreamSource Starting parameter was None. This should not happen.");

            stream_start.Request()?.SetActualStartPosition(TimeSpan { Duration: 0 })?;
            Ok(())
        }))?;

        let (frame_sender, frame_receiver) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();

        let (audio_sender, audio_receiver) =
            mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let frame_notify = Arc::new((Mutex::new(false), Condvar::new()));
        let audio_notify = Arc::new((Mutex::new(false), Condvar::new()));

        let sample_requested = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new({
            let frame_receiver = frame_receiver;
            let frame_notify = frame_notify.clone();

            let audio_receiver = audio_receiver;
            let audio_notify = audio_notify.clone();

            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None. This should not happen.",
                );

                if sample_requested
                    .Request()?
                    .StreamDescriptor()?
                    .cast::<AudioStreamDescriptor>()
                    .is_ok()
                {
                    if is_audio_disabled {
                        sample_requested.Request()?.SetSample(None)?;

                        return Ok(());
                    }

                    let audio = match audio_receiver.recv() {
                        Ok(audio) => audio,
                        Err(e) => panic!("Failed to receive audio from the audio sender: {e}"),
                    };

                    match audio {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                AudioEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
                                }
                            };

                            sample_requested.Request()?.SetSample(&sample)?;
                        }
                        None => {
                            sample_requested.Request()?.SetSample(None)?;
                        }
                    }

                    let (lock, cvar) = &*audio_notify;
                    *lock.lock() = true;
                    cvar.notify_one();
                } else {
                    if is_video_disabled {
                        sample_requested.Request()?.SetSample(None)?;

                        return Ok(());
                    }

                    let frame = match frame_receiver.recv() {
                        Ok(frame) => frame,
                        Err(e) => panic!("Failed to receive a frame from the frame sender: {e}"),
                    };

                    match frame {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                VideoEncoderSource::DirectX(surface) => {
                                    MediaStreamSample::CreateFromDirect3D11Surface(
                                        &surface.0, timestamp,
                                    )?
                                }
                                VideoEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
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
                }

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
            first_timestamp: None,
            frame_sender,
            audio_sender,
            sample_requested,
            media_stream_source,
            starting,
            transcode_thread: Some(transcode_thread),
            frame_notify,
            audio_notify,
            error_notify,
            is_video_disabled,
            is_audio_disabled,
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
    #[inline]
    pub fn send_frame(&mut self, frame: &mut Frame) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        let timestamp = match self.first_timestamp {
            Some(timestamp) => {
                TimeSpan { Duration: frame.timestamp().Duration - timestamp.Duration }
            }
            None => {
                let timestamp = frame.timestamp();
                self.first_timestamp = Some(timestamp);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            timestamp,
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
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Sends a video frame with audio to the video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `frame` - A mutable reference to the `Frame` to be encoded.
    /// * `audio_buffer` - A reference to the audio byte slice to be encoded.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    #[inline]
    pub fn send_frame_with_audio(
        &mut self,
        frame: &mut Frame,
        audio_buffer: &[u8],
    ) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        if self.is_audio_disabled {
            return Err(VideoEncoderError::AudioDisabled);
        }

        let timestamp = match self.first_timestamp {
            Some(timestamp) => {
                TimeSpan { Duration: frame.timestamp().Duration - timestamp.Duration }
            }
            None => {
                let timestamp = frame.timestamp();
                self.first_timestamp = Some(timestamp);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            timestamp,
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
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        self.audio_sender.send(Some((
            AudioEncoderSource::Buffer((
                SendDirectX::new(audio_buffer.as_ptr()),
                audio_buffer.len(),
            )),
            timestamp,
        )))?;

        let (lock, cvar) = &*self.audio_notify;
        let mut processed = lock.lock();
        if !*processed {
            cvar.wait(&mut processed);
        }
        *processed = false;
        drop(processed);

        if self.error_notify.load(atomic::Ordering::Relaxed) {
            if let Some(transcode_thread) = self.transcode_thread.take() {
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Sends a video frame to the video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `buffer` - A reference to the frame byte slice to be encoded. The Windows API expects this to be BGRA and bottom-to-top.
    /// * `timestamp` - The timestamp of the frame, in 100-nanosecond units.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    #[inline]
    pub fn send_frame_buffer(
        &mut self,
        buffer: &[u8],
        timestamp: i64,
    ) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        let frame_timestamp = timestamp;
        let timestamp = match self.first_timestamp {
            Some(timestamp) => TimeSpan { Duration: frame_timestamp - timestamp.Duration },
            None => {
                let timestamp = frame_timestamp;
                self.first_timestamp = Some(TimeSpan { Duration: timestamp });
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::Buffer((SendDirectX::new(buffer.as_ptr()), buffer.len())),
            timestamp,
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
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Sends a video audio to the video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `buffer` - A reference to the audio byte slice to be encoded.
    /// * `timestamp` - The timestamp of the audio buffer, in 100-nanosecond units.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    #[inline]
    pub fn send_audio_buffer(
        &mut self,
        buffer: &[u8],
        timestamp: i64,
    ) -> Result<(), VideoEncoderError> {
        if self.is_audio_disabled {
            return Err(VideoEncoderError::AudioDisabled);
        }

        let audio_timestamp = timestamp;
        let timestamp = match self.first_timestamp {
            Some(timestamp) => TimeSpan { Duration: audio_timestamp - timestamp.Duration },
            None => {
                let timestamp = audio_timestamp;
                self.first_timestamp = Some(TimeSpan { Duration: timestamp });
                TimeSpan { Duration: 0 }
            }
        };

        self.audio_sender.send(Some((
            AudioEncoderSource::Buffer((SendDirectX::new(buffer.as_ptr()), buffer.len())),
            timestamp,
        )))?;

        let (lock, cvar) = &*self.audio_notify;
        let mut processed = lock.lock();
        if !*processed {
            cvar.wait(&mut processed);
        }
        *processed = false;
        drop(processed);

        if self.error_notify.load(atomic::Ordering::Relaxed) {
            if let Some(transcode_thread) = self.transcode_thread.take() {
                transcode_thread.join().expect("Failed to join transcode thread")?;
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
    #[inline]
    pub fn finish(mut self) -> Result<(), VideoEncoderError> {
        self.frame_sender.send(None)?;
        self.audio_sender.send(None)?;

        if let Some(transcode_thread) = self.transcode_thread.take() {
            transcode_thread.join().expect("Failed to join transcode thread")?;
        }

        self.media_stream_source.RemoveStarting(self.starting)?;
        self.media_stream_source.RemoveSampleRequested(self.sample_requested)?;

        Ok(())
    }
}

impl Drop for VideoEncoder {
    #[inline]
    fn drop(&mut self) {
        let _ = self.frame_sender.send(None);

        if let Some(transcode_thread) = self.transcode_thread.take() {
            let _ = transcode_thread.join();
        }
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for VideoEncoder {}

/// The `StreamingVideoEncoder` struct is used to encode video frames and stream them in real-time
/// without writing to files. It uses a callback mechanism to deliver encoded frames.
pub struct StreamingVideoEncoder {
    first_timestamp: Option<TimeSpan>,
    frame_sender: mpsc::Sender<Option<(VideoEncoderSource, TimeSpan)>>,
    audio_sender: mpsc::Sender<Option<(AudioEncoderSource, TimeSpan)>>,
    sample_requested: i64,
    media_stream_source: MediaStreamSource,
    starting: i64,
    transcode_thread: Option<JoinHandle<Result<(), VideoEncoderError>>>,
    frame_notify: Arc<(Mutex<bool>, Condvar)>,
    audio_notify: Arc<(Mutex<bool>, Condvar)>,
    error_notify: Arc<AtomicBool>,
    is_video_disabled: bool,
    is_audio_disabled: bool,
    callback: Arc<Mutex<Box<dyn FrameCallback>>>,
    encoded_frame_sender: mpsc::Sender<EncodedFrame>,
    encoded_audio_sender: mpsc::Sender<EncodedAudioFrame>,
}

impl StreamingVideoEncoder {
    /// Creates a new `StreamingVideoEncoder` instance with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `video_settings` - The video encoder settings.
    /// * `audio_settings` - The audio encoder settings.
    /// * `container_settings` - The container settings.
    /// * `callback` - The callback for handling encoded frames.
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the `StreamingVideoEncoder` instance if successful, or a
    /// `VideoEncoderError` if an error occurs.
    #[inline]
    pub fn new(
        video_settings: VideoSettingsBuilder,
        audio_settings: AudioSettingsBuilder,
        container_settings: ContainerSettingsBuilder,
        callback: Box<dyn FrameCallback>,
    ) -> Result<Self, VideoEncoderError> {
        let media_encoding_profile = MediaEncodingProfile::new()?;

        let (video_encoding_properties, is_video_disabled) = video_settings.build()?;
        media_encoding_profile.SetVideo(&video_encoding_properties)?;
        let (audio_encoding_properties, is_audio_disabled) = audio_settings.build()?;
        media_encoding_profile.SetAudio(&audio_encoding_properties)?;
        let container_encoding_properties = container_settings.build()?;
        media_encoding_profile.SetContainer(&container_encoding_properties)?;

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            video_encoding_properties.Width()?,
            video_encoding_properties.Height()?,
        )?;
        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let audio_encoding_properties = AudioEncodingProperties::CreateAac(
            audio_encoding_properties.SampleRate()?,
            audio_encoding_properties.ChannelCount()?,
            audio_encoding_properties.Bitrate()?,
        )?;
        let audio_stream_descriptor = AudioStreamDescriptor::Create(&audio_encoding_properties)?;

        let media_stream_source = MediaStreamSource::CreateFromDescriptors(
            &video_stream_descriptor,
            &audio_stream_descriptor,
        )?;
        media_stream_source.SetBufferTime(TimeSpan::default())?;

        let starting = media_stream_source.Starting(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceStartingEventArgs,
        >::new(move |_, stream_start| {
            let stream_start = stream_start
                .as_ref()
                .expect("MediaStreamSource Starting parameter was None. This should not happen.");

            stream_start.Request()?.SetActualStartPosition(TimeSpan { Duration: 0 })?;
            Ok(())
        }))?;

        let (frame_sender, frame_receiver) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();

        let (audio_sender, audio_receiver) =
            mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let frame_notify = Arc::new((Mutex::new(false), Condvar::new()));
        let audio_notify = Arc::new((Mutex::new(false), Condvar::new()));

        let (encoded_frame_sender, encoded_frame_receiver) = mpsc::channel::<EncodedFrame>();
        let (encoded_audio_sender, encoded_audio_receiver) = mpsc::channel::<EncodedAudioFrame>();

        let callback = Arc::new(Mutex::new(callback));

        let sample_requested = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new({
            let frame_receiver = frame_receiver;
            let frame_notify = frame_notify.clone();
            let audio_receiver = audio_receiver;
            let audio_notify = audio_notify.clone();
            let callback = callback.clone();
            let encoded_frame_sender = encoded_frame_sender.clone();
            let encoded_audio_sender = encoded_audio_sender.clone();

            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None. This should not happen.",
                );

                if sample_requested
                    .Request()?
                    .StreamDescriptor()?
                    .cast::<AudioStreamDescriptor>()
                    .is_ok()
                {
                    if is_audio_disabled {
                        sample_requested.Request()?.SetSample(None)?;
                        return Ok(());
                    }

                    let audio = match audio_receiver.recv() {
                        Ok(audio) => audio,
                        Err(e) => panic!("Failed to receive audio from the audio sender: {e}"),
                    };

                    match audio {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                AudioEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
                                }
                            };

                            // Extract encoded audio data and send to callback
                            let audio_data = sample.Buffer()?;
                            let audio_buffer = CryptographicBuffer::CopyToByteArray(&audio_data)?;
                            
                            let encoded_audio = EncodedAudioFrame {
                                data: audio_buffer.to_vec(),
                                timestamp: timestamp.Duration,
                                sample_count: audio_encoding_properties.SampleRate()? / 1000, // Approximate
                            };

                            // Send to callback
                            if let Err(e) = encoded_audio_sender.send(encoded_audio) {
                                eprintln!("Failed to send encoded audio frame: {}", e);
                            }

                            sample_requested.Request()?.SetSample(&sample)?;
                        }
                        None => {
                            sample_requested.Request()?.SetSample(None)?;
                        }
                    }

                    let (lock, cvar) = &*audio_notify;
                    *lock.lock() = true;
                    cvar.notify_one();
                } else {
                    if is_video_disabled {
                        sample_requested.Request()?.SetSample(None)?;
                        return Ok(());
                    }

                    let frame = match frame_receiver.recv() {
                        Ok(frame) => frame,
                        Err(e) => panic!("Failed to receive a frame from the frame sender: {e}"),
                    };

                    match frame {
                        Some((source, timestamp)) => {
                            let sample = match source {
                                VideoEncoderSource::DirectX(surface) => {
                                    MediaStreamSample::CreateFromDirect3D11Surface(
                                        &surface.0, timestamp,
                                    )?
                                }
                                VideoEncoderSource::Buffer(buffer_data) => {
                                    let buffer = buffer_data.0;
                                    let buffer =
                                        unsafe { slice::from_raw_parts(buffer.0, buffer_data.1) };
                                    let buffer = CryptographicBuffer::CreateFromByteArray(buffer)?;
                                    MediaStreamSample::CreateFromBuffer(&buffer, timestamp)?
                                }
                            };

                            // Extract encoded video data and send to callback
                            let video_data = sample.Buffer()?;
                            let video_buffer = CryptographicBuffer::CopyToByteArray(&video_data)?;
                            
                            let encoded_frame = EncodedFrame {
                                data: video_buffer.to_vec(),
                                timestamp: timestamp.Duration,
                                frame_type: FrameType::DeltaFrame, // Default, could be determined from sample properties
                                width: video_encoding_properties.Width()?,
                                height: video_encoding_properties.Height()?,
                            };

                            // Send to callback
                            if let Err(e) = encoded_frame_sender.send(encoded_frame) {
                                eprintln!("Failed to send encoded video frame: {}", e);
                            }

                            sample_requested.Request()?.SetSample(&sample)?;
                        }
                        None => {
                            sample_requested.Request()?.SetSample(None)?;
                        }
                    }

                    let (lock, cvar) = &*frame_notify;
                    *lock.lock() = true;
                    cvar.notify_one();
                }

                Ok(())
            }
        }))?;

        let media_transcoder = MediaTranscoder::new()?;
        media_transcoder.SetHardwareAccelerationEnabled(true)?;

        // Create an in-memory stream for transcoding
        let stream = InMemoryRandomAccessStream::new()?;

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
            let callback = callback.clone();

            move || -> Result<(), VideoEncoderError> {
                // Notify callback that stream is starting
                if let Err(e) = callback.lock().on_stream_start() {
                    eprintln!("Failed to notify stream start: {}", e);
                }

                let result = transcode.TranscodeAsync();

                if result.is_err() {
                    error_notify.store(true, atomic::Ordering::Relaxed);
                }

                result?.get()?;

                // Notify callback that stream is ending
                if let Err(e) = callback.lock().on_stream_end() {
                    eprintln!("Failed to notify stream end: {}", e);
                }

                drop(media_transcoder);

                Ok(())
            }
        });

        // Start callback processing thread
        let callback_thread = thread::spawn({
            let callback = callback.clone();
            move || {
                while let Ok(encoded_frame) = encoded_frame_receiver.recv() {
                    if let Err(e) = callback.lock().on_video_frame(encoded_frame) {
                        eprintln!("Failed to process video frame: {}", e);
                    }
                }
            }
        });

        let audio_callback_thread = thread::spawn({
            let callback = callback.clone();
            move || {
                while let Ok(encoded_audio) = encoded_audio_receiver.recv() {
                    if let Err(e) = callback.lock().on_audio_frame(encoded_audio) {
                        eprintln!("Failed to process audio frame: {}", e);
                    }
                }
            }
        });

        Ok(Self {
            first_timestamp: None,
            frame_sender,
            audio_sender,
            sample_requested,
            media_stream_source,
            starting,
            transcode_thread: Some(transcode_thread),
            frame_notify,
            audio_notify,
            error_notify,
            is_video_disabled,
            is_audio_disabled,
            callback,
            encoded_frame_sender,
            encoded_audio_sender,
        })
    }

    /// Sends a video frame to the streaming video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `frame` - A mutable reference to the `Frame` to be encoded.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    #[inline]
    pub fn send_frame(&mut self, frame: &mut Frame) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        let timestamp = match self.first_timestamp {
            Some(timestamp) => {
                TimeSpan { Duration: frame.timestamp().Duration - timestamp.Duration }
            }
            None => {
                let timestamp = frame.timestamp();
                self.first_timestamp = Some(timestamp);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            timestamp,
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
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        Ok(())
    }

    /// Sends a video frame with audio to the streaming video encoder for encoding.
    ///
    /// # Arguments
    ///
    /// * `frame` - A mutable reference to the `Frame` to be encoded.
    /// * `audio_buffer` - A reference to the audio byte slice to be encoded.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the frame is successfully sent for encoding, or a `VideoEncoderError`
    /// if an error occurs.
    #[inline]
    pub fn send_frame_with_audio(
        &mut self,
        frame: &mut Frame,
        audio_buffer: &[u8],
    ) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        if self.is_audio_disabled {
            return Err(VideoEncoderError::AudioDisabled);
        }

        let timestamp = match self.first_timestamp {
            Some(timestamp) => {
                TimeSpan { Duration: frame.timestamp().Duration - timestamp.Duration }
            }
            None => {
                let timestamp = frame.timestamp();
                self.first_timestamp = Some(timestamp);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            timestamp,
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
                transcode_thread.join().expect("Failed to join transcode thread")?;
            }
        }

        self.audio_sender.send(Some((
            AudioEncoderSource::Buffer((
                SendDirectX::new(audio_buffer.as_ptr()),
                audio_buffer.len(),
            )),
            timestamp,
        )))?;

        let (lock, cvar) = &*self.audio_notify;
        let mut processed = lock.lock();
        if !*processed {
            cvar.wait(&mut processed);
        }
        *processed = false;
        drop(processed);

        if self.error_notify.load(atomic::Ordering::Relaxed) {
            if let Some(transcode_thread) = self.transcode_thread.take() {
                transcode_thread.join().expect("Failed to join transcode thread")?;
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
    #[inline]
    pub fn finish(mut self) -> Result<(), VideoEncoderError> {
        self.frame_sender.send(None)?;
        self.audio_sender.send(None)?;

        if let Some(transcode_thread) = self.transcode_thread.take() {
            transcode_thread.join().expect("Failed to join transcode thread")?;
        }

        self.media_stream_source.RemoveStarting(self.starting)?;
        self.media_stream_source.RemoveSampleRequested(self.sample_requested)?;

        Ok(())
    }
}

impl Drop for StreamingVideoEncoder {
    #[inline]
    fn drop(&mut self) {
        let _ = self.frame_sender.send(None);

        if let Some(transcode_thread) = self.transcode_thread.take() {
            let _ = transcode_thread.join();
        }
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for StreamingVideoEncoder {}
