use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::{self, AtomicBool};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use parking_lot::Mutex; // Only used to serialize recv() per stream.
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
use windows::System::Threading::{ThreadPool, WorkItemHandler, WorkItemOptions, WorkItemPriority};
use windows::core::{HSTRING, Interface};

use crate::d3d11::SendDirectX;
use crate::frame::{Frame, ImageFormat};
use crate::settings::ColorFormat;

type VideoFrameReceiver = Arc<Mutex<mpsc::Receiver<Option<(VideoEncoderSource, TimeSpan)>>>>;
type AudioFrameReceiver = Arc<Mutex<mpsc::Receiver<Option<(AudioEncoderSource, TimeSpan)>>>>;

#[derive(thiserror::Error, Eq, PartialEq, Clone, Debug)]
pub enum ImageEncoderError {
    #[error("This color format is not supported for saving as an image")]
    UnsupportedFormat,
    #[error("Windows API error: {0}")]
    WindowsError(#[from] windows::core::Error),
    #[error("Integer conversion error: {0}")]
    IntConversionError(#[from] std::num::TryFromIntError),
}

/// Encodes image buffers into image bytes with a specified image format and color format.
pub struct ImageEncoder {
    format: ImageFormat,
    color_format: ColorFormat,
}

impl ImageEncoder {
    #[must_use]
    #[inline]
    pub const fn new(format: ImageFormat, color_format: ColorFormat) -> Self {
        Self { format, color_format }
    }

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
        let encoder = BitmapEncoder::CreateAsync(encoder, &stream)?.join()?;

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

        encoder.FlushAsync()?.join()?;

        let buffer = Buffer::Create(u32::try_from(stream.Size()?)?)?;
        stream.ReadAsync(&buffer, buffer.Capacity()?, InputStreamOptions::None)?.join()?;

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

/// Video sources.
/// - DirectX surfaces: COM-refcounted; holding the pointer is enough.
/// - Buffer path now **owns** the bytes (Vec<u8>) so callers can return immediately.
pub enum VideoEncoderSource {
    DirectX(SendDirectX<IDirect3DSurface>),
    Buffer(Vec<u8>),
}

/// Audio sources that now **own** the bytes.
pub enum AudioEncoderSource {
    Buffer(Vec<u8>),
}

/// Builder for configuring video encoder settings.
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
            bitrate: 15_000_000,
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
    pub const fn pixel_aspect_ratio(mut self, par: (u32, u32)) -> Self {
        self.pixel_aspect_ratio = par;
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

/// Builder for configuring audio encoder settings.
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
            bitrate: 192_000,
            channel_count: 2,
            sample_rate: 48_000,
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

/// Builder for configuring container settings.
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

/// Video encoder subtypes.
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

/// Audio encoder subtypes.
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

/// Container subtypes.
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

/// Encodes video frames (and optional audio) and writes them to a file or stream.
pub struct VideoEncoder {
    // Video timing
    first_timestamp: Option<TimeSpan>,

    // Channels
    frame_sender: mpsc::Sender<Option<(VideoEncoderSource, TimeSpan)>>,
    audio_sender: mpsc::Sender<Option<(AudioEncoderSource, TimeSpan)>>,

    // MSS event tokens
    sample_requested: i64,
    media_stream_source: MediaStreamSource,
    starting: i64,

    // Transcode worker
    transcode_thread: Option<JoinHandle<Result<(), VideoEncoderError>>>,
    error_notify: Arc<AtomicBool>,

    // Feature toggles
    is_video_disabled: bool,
    is_audio_disabled: bool,

    // --- NEW: audio clock & format bookkeeping (monotonic timing) ---
    audio_sample_rate: u32,  // Hz (frames per second)
    audio_block_align: u32,  // bytes per interleaved sample frame (channels * (bits/8))
    audio_samples_sent: u64, // number of sample frames (not bytes) emitted so far
}

impl VideoEncoder {
    fn attach_sample_requested_handlers(
        media_stream_source: &MediaStreamSource,
        is_video_disabled: bool,
        is_audio_disabled: bool,
        frame_receiver: VideoFrameReceiver,
        audio_receiver: AudioFrameReceiver,
        audio_block_align: u32,
        audio_sample_rate: u32,
    ) -> Result<i64, VideoEncoderError> {
        let token = media_stream_source.SampleRequested(&TypedEventHandler::<
            MediaStreamSource,
            MediaStreamSourceSampleRequestedEventArgs,
        >::new(
            move |_, sample_requested| {
                let sample_requested = sample_requested.as_ref().expect(
                    "MediaStreamSource SampleRequested parameter was None. This should not happen.",
                );

                let request = sample_requested.Request()?;
                let is_audio = request.StreamDescriptor()?.cast::<AudioStreamDescriptor>().is_ok();

                // Always offload blocking work to the thread pool; never block the MSS event thread.
                let deferral = request.GetDeferral()?;

                if is_audio {
                    if is_audio_disabled {
                        request.SetSample(None)?;
                        deferral.Complete()?;
                    } else {
                        let request_clone = request;
                        let audio_receiver = audio_receiver.clone();
                        ThreadPool::RunWithPriorityAndOptionsAsync(
                            &WorkItemHandler::new(move |_| {
                                let value = audio_receiver.lock().recv();
                                match value {
                                    Ok(Some((source, timestamp))) => {
                                        let sample = match source {
                                            AudioEncoderSource::Buffer(bytes) => {
                                                let buf = CryptographicBuffer::CreateFromByteArray(
                                                    &bytes,
                                                )?;
                                                let sample = MediaStreamSample::CreateFromBuffer(
                                                    &buf, timestamp,
                                                )?;
                                                // Duration = (frames / sample_rate) in 100ns ticks
                                                // frames = bytes / block_align
                                                let frames =
                                                    (bytes.len() as u32) / audio_block_align;
                                                let duration_ticks = (frames as i64)
                                                    * 10_000_000i64
                                                    / (audio_sample_rate as i64);
                                                sample.SetDuration(TimeSpan {
                                                    Duration: duration_ticks,
                                                })?;
                                                sample
                                            }
                                        };
                                        request_clone.SetSample(&sample)?;
                                    }
                                    Ok(None) | Err(_) => {
                                        request_clone.SetSample(None)?;
                                    }
                                }
                                deferral.Complete()?;
                                Ok(())
                            }),
                            WorkItemPriority::Normal,
                            WorkItemOptions::None, // None finishes tail work faster
                        )?;
                    }
                } else if is_video_disabled {
                    request.SetSample(None)?;
                    deferral.Complete()?;
                } else {
                    let request_clone = request;
                    let frame_receiver = frame_receiver.clone();
                    ThreadPool::RunWithPriorityAndOptionsAsync(
                        &WorkItemHandler::new(move |_| {
                            let value = frame_receiver.lock().recv();
                            match value {
                                Ok(Some((source, timestamp))) => {
                                    let sample = match source {
                                        VideoEncoderSource::DirectX(surface) => {
                                            MediaStreamSample::CreateFromDirect3D11Surface(
                                                &surface.0, timestamp,
                                            )?
                                        }
                                        VideoEncoderSource::Buffer(bytes) => {
                                            let buf =
                                                CryptographicBuffer::CreateFromByteArray(&bytes)?;
                                            MediaStreamSample::CreateFromBuffer(&buf, timestamp)?
                                        }
                                    };
                                    request_clone.SetSample(&sample)?;
                                }
                                Ok(None) | Err(_) => {
                                    request_clone.SetSample(None)?;
                                }
                            }
                            deferral.Complete()?;
                            Ok(())
                        }),
                        WorkItemPriority::Normal,
                        WorkItemOptions::None,
                    )?;
                }

                Ok(())
            },
        ))?;
        Ok(token)
    }

    /// Creates a new `VideoEncoder` that writes to a file path.
    #[inline]
    pub fn new<P: AsRef<Path>>(
        video_settings: VideoSettingsBuilder,
        audio_settings: AudioSettingsBuilder,
        container_settings: ContainerSettingsBuilder,
        path: P,
    ) -> Result<Self, VideoEncoderError> {
        let path = path.as_ref();
        let media_encoding_profile = MediaEncodingProfile::new()?;

        let (video_encoding_properties_cfg, is_video_disabled) = video_settings.build()?;
        media_encoding_profile.SetVideo(&video_encoding_properties_cfg)?;
        let (audio_encoding_properties_cfg, is_audio_disabled) = audio_settings.build()?;
        media_encoding_profile.SetAudio(&audio_encoding_properties_cfg)?;
        let container_encoding_properties = container_settings.build()?;
        media_encoding_profile.SetContainer(&container_encoding_properties)?;

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            video_encoding_properties_cfg.Width()?,
            video_encoding_properties_cfg.Height()?,
        )?;
        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        // Stream descriptor uses PCM; the profile still encodes to AAC/OPUS/etc.
        let audio_desc_props = AudioEncodingProperties::CreatePcm(
            audio_encoding_properties_cfg.SampleRate()?,
            audio_encoding_properties_cfg.ChannelCount()?,
            audio_encoding_properties_cfg.BitsPerSample()?,
        )?;
        let audio_stream_descriptor = AudioStreamDescriptor::Create(&audio_desc_props)?;

        // Compute audio block align/sample-rate for monotonic clock
        let audio_sr = audio_desc_props.SampleRate()?;
        let audio_ch = audio_desc_props.ChannelCount()?;
        let audio_bps = audio_desc_props.BitsPerSample()?;
        let audio_block_align = (audio_bps / 8) * audio_ch;

        let media_stream_source = MediaStreamSource::CreateFromDescriptors(
            &video_stream_descriptor,
            &audio_stream_descriptor,
        )?;
        // Keep a modest buffer (30ms)
        media_stream_source.SetBufferTime(Duration::from_millis(30).into())?;

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

        let (frame_sender, frame_receiver_raw) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();
        let (audio_sender, audio_receiver_raw) =
            mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let frame_receiver = Arc::new(Mutex::new(frame_receiver_raw));
        let audio_receiver = Arc::new(Mutex::new(audio_receiver_raw));

        let sample_requested = Self::attach_sample_requested_handlers(
            &media_stream_source,
            is_video_disabled,
            is_audio_disabled,
            frame_receiver,
            audio_receiver,
            audio_block_align,
            audio_sr,
        )?;

        let media_transcoder = MediaTranscoder::new()?;
        media_transcoder.SetHardwareAccelerationEnabled(true)?;

        File::create(path)?;
        let path = fs::canonicalize(path)?.to_string_lossy()[4..].to_string();
        let path = Path::new(&path);
        let path = &HSTRING::from(path.as_os_str().to_os_string());

        let file = StorageFile::GetFileFromPathAsync(path)?.join()?;
        let media_stream_output = file.OpenAsync(FileAccessMode::ReadWrite)?.join()?;

        let transcode = media_transcoder
            .PrepareMediaStreamSourceTranscodeAsync(
                &media_stream_source,
                &media_stream_output,
                &media_encoding_profile,
            )?
            .join()?;

        let error_notify = Arc::new(AtomicBool::new(false));
        let transcode_thread = thread::spawn({
            let error_notify = error_notify.clone();
            move || -> Result<(), VideoEncoderError> {
                let result = transcode.TranscodeAsync();
                if result.is_err() {
                    error_notify.store(true, atomic::Ordering::Relaxed);
                }
                result?.join()?;
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
            error_notify,
            is_video_disabled,
            is_audio_disabled,
            audio_sample_rate: audio_sr,
            audio_block_align,
            audio_samples_sent: 0,
        })
    }

    /// Creates a new `VideoEncoder` that writes to the given stream.
    #[inline]
    pub fn new_from_stream(
        video_settings: VideoSettingsBuilder,
        audio_settings: AudioSettingsBuilder,
        container_settings: ContainerSettingsBuilder,
        stream: IRandomAccessStream,
    ) -> Result<Self, VideoEncoderError> {
        let media_encoding_profile = MediaEncodingProfile::new()?;

        let (video_encoding_properties_cfg, is_video_disabled) = video_settings.build()?;
        media_encoding_profile.SetVideo(&video_encoding_properties_cfg)?;
        let (audio_encoding_properties_cfg, is_audio_disabled) = audio_settings.build()?;
        media_encoding_profile.SetAudio(&audio_encoding_properties_cfg)?;
        let container_encoding_properties = container_settings.build()?;
        media_encoding_profile.SetContainer(&container_encoding_properties)?;

        let video_encoding_properties = VideoEncodingProperties::CreateUncompressed(
            &MediaEncodingSubtypes::Bgra8()?,
            video_encoding_properties_cfg.Width()?,
            video_encoding_properties_cfg.Height()?,
        )?;
        let video_stream_descriptor = VideoStreamDescriptor::Create(&video_encoding_properties)?;

        let audio_desc_props = AudioEncodingProperties::CreatePcm(
            audio_encoding_properties_cfg.SampleRate()?,
            audio_encoding_properties_cfg.ChannelCount()?,
            audio_encoding_properties_cfg.BitsPerSample()?,
        )?;
        let audio_stream_descriptor = AudioStreamDescriptor::Create(&audio_desc_props)?;

        // Monotonic audio timing parameters
        let audio_sr = audio_desc_props.SampleRate()?;
        let audio_ch = audio_desc_props.ChannelCount()?;
        let audio_bps = audio_desc_props.BitsPerSample()?;
        let audio_block_align = (audio_bps / 8) * audio_ch;

        let media_stream_source = MediaStreamSource::CreateFromDescriptors(
            &video_stream_descriptor,
            &audio_stream_descriptor,
        )?;
        // CHANGED: use 30ms buffer (was 0)
        media_stream_source.SetBufferTime(Duration::from_millis(30).into())?;

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

        let (frame_sender, frame_receiver_raw) =
            mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();
        let (audio_sender, audio_receiver_raw) =
            mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let frame_receiver = Arc::new(Mutex::new(frame_receiver_raw));
        let audio_receiver = Arc::new(Mutex::new(audio_receiver_raw));

        let sample_requested = Self::attach_sample_requested_handlers(
            &media_stream_source,
            is_video_disabled,
            is_audio_disabled,
            frame_receiver,
            audio_receiver,
            audio_block_align,
            audio_sr,
        )?;

        let media_transcoder = MediaTranscoder::new()?;
        media_transcoder.SetHardwareAccelerationEnabled(true)?;

        let transcode = media_transcoder
            .PrepareMediaStreamSourceTranscodeAsync(
                &media_stream_source,
                &stream,
                &media_encoding_profile,
            )?
            .join()?;

        let error_notify = Arc::new(AtomicBool::new(false));
        let transcode_thread = thread::spawn({
            let error_notify = error_notify.clone();
            move || -> Result<(), VideoEncoderError> {
                let result = transcode.TranscodeAsync();
                if result.is_err() {
                    error_notify.store(true, atomic::Ordering::Relaxed);
                }
                result?.join()?;
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
            error_notify,
            is_video_disabled,
            is_audio_disabled,
            audio_sample_rate: audio_sr,
            audio_block_align,
            audio_samples_sent: 0,
        })
    }

    /// Sends a video frame (DirectX). Returns immediately.
    #[inline]
    pub fn send_frame(&mut self, frame: &mut Frame) -> Result<(), VideoEncoderError> {
        if self.is_video_disabled {
            return Err(VideoEncoderError::VideoDisabled);
        }

        let timestamp = match self.first_timestamp {
            Some(t0) => TimeSpan { Duration: frame.timestamp()?.Duration - t0.Duration },
            None => {
                let ts = frame.timestamp()?;
                self.first_timestamp = Some(ts);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            timestamp,
        )))?;

        if self.error_notify.load(atomic::Ordering::Relaxed)
            && let Some(t) = self.transcode_thread.take()
        {
            t.join().expect("Failed to join transcode thread")?;
        }

        Ok(())
    }

    /// Sends a video frame and an audio buffer (owned). Returns immediately.
    /// Audio timestamp is derived from total samples sent so far (monotonic).
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

        // Video timestamp based on capture timestamps (as before)
        let video_ts = match self.first_timestamp {
            Some(t0) => TimeSpan { Duration: frame.timestamp()?.Duration - t0.Duration },
            None => {
                let ts = frame.timestamp()?;
                self.first_timestamp = Some(ts);
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((
            VideoEncoderSource::DirectX(SendDirectX::new(unsafe {
                frame.as_raw_surface().clone()
            })),
            video_ts,
        )))?;

        // Audio timestamp from running sample count
        let frames_in_buf = (audio_buffer.len() as u32) / self.audio_block_align;
        let audio_ts_ticks =
            ((self.audio_samples_sent as i128) * 10_000_000i128) / (self.audio_sample_rate as i128);
        let audio_ts = TimeSpan { Duration: audio_ts_ticks as i64 };

        self.audio_sender
            .send(Some((AudioEncoderSource::Buffer(audio_buffer.to_vec()), audio_ts)))?;

        // Advance counter after stamping
        self.audio_samples_sent = self.audio_samples_sent.saturating_add(frames_in_buf as u64);

        if self.error_notify.load(atomic::Ordering::Relaxed)
            && let Some(t) = self.transcode_thread.take()
        {
            t.join().expect("Failed to join transcode thread")?;
        }

        Ok(())
    }

    /// Sends a raw frame buffer (owned inside). Returns immediately.
    /// Windows expects BGRA and bottom-to-top layout for this path.
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
            Some(t0) => TimeSpan { Duration: frame_timestamp - t0.Duration },
            None => {
                self.first_timestamp = Some(TimeSpan { Duration: frame_timestamp });
                TimeSpan { Duration: 0 }
            }
        };

        self.frame_sender.send(Some((VideoEncoderSource::Buffer(buffer.to_vec()), timestamp)))?;

        if self.error_notify.load(atomic::Ordering::Relaxed)
            && let Some(t) = self.transcode_thread.take()
        {
            t.join().expect("Failed to join transcode thread")?;
        }

        Ok(())
    }

    /// Sends an audio buffer (owned inside). Returns immediately.
    /// NOTE: The provided `timestamp` is ignored; we use a monotonic audio clock.
    #[inline]
    pub fn send_audio_buffer(
        &mut self,
        buffer: &[u8],
        _timestamp: i64, // ignored to guarantee monotonic audio timing
    ) -> Result<(), VideoEncoderError> {
        if self.is_audio_disabled {
            return Err(VideoEncoderError::AudioDisabled);
        }

        let frames_in_buf = (buffer.len() as u32) / self.audio_block_align;
        let audio_ts_ticks =
            ((self.audio_samples_sent as i128) * 10_000_000i128) / (self.audio_sample_rate as i128);
        let timestamp = TimeSpan { Duration: audio_ts_ticks as i64 };

        self.audio_sender.send(Some((AudioEncoderSource::Buffer(buffer.to_vec()), timestamp)))?;

        self.audio_samples_sent = self.audio_samples_sent.saturating_add(frames_in_buf as u64);

        if self.error_notify.load(atomic::Ordering::Relaxed)
            && let Some(t) = self.transcode_thread.take()
        {
            t.join().expect("Failed to join transcode thread")?;
        }

        Ok(())
    }

    /// Finishes the encoding and performs any necessary cleanup.
    #[inline]
    pub fn finish(mut self) -> Result<(), VideoEncoderError> {
        // 1) Signal EOS on both streams.
        let _ = self.frame_sender.send(None);
        let _ = self.audio_sender.send(None);

        // 2) **Close the channels** so any further recv() returns Err immediately.
        //    We replace the fields with dummy senders and drop the originals now.
        {
            let (dummy_tx_v, _dummy_rx_v) =
                mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();
            let (dummy_tx_a, _dummy_rx_a) =
                mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

            let old_v = std::mem::replace(&mut self.frame_sender, dummy_tx_v);
            let old_a = std::mem::replace(&mut self.audio_sender, dummy_tx_a);
            drop(old_v);
            drop(old_a);
        }

        // 3) Wait for the transcoder to flush and finalize.
        if let Some(transcode_thread) = self.transcode_thread.take() {
            transcode_thread.join().expect("Failed to join transcode thread")?;
        }

        // 4) Unhook events after pipeline has completed.
        self.media_stream_source.RemoveStarting(self.starting)?;
        self.media_stream_source.RemoveSampleRequested(self.sample_requested)?;

        Ok(())
    }
}

impl Drop for VideoEncoder {
    #[inline]
    fn drop(&mut self) {
        // Try to signal EOS, then **close** the channels before waiting.
        let _ = self.frame_sender.send(None);
        let _ = self.audio_sender.send(None);

        // Close channels early in Drop too (same trick as in finish()).
        let (dummy_tx_v, _dummy_rx_v) = mpsc::channel::<Option<(VideoEncoderSource, TimeSpan)>>();
        let (dummy_tx_a, _dummy_rx_a) = mpsc::channel::<Option<(AudioEncoderSource, TimeSpan)>>();

        let old_v = std::mem::replace(&mut self.frame_sender, dummy_tx_v);
        let old_a = std::mem::replace(&mut self.audio_sender, dummy_tx_a);
        drop(old_v);
        drop(old_a);

        if let Some(transcode_thread) = self.transcode_thread.take() {
            let _ = transcode_thread.join();
        }

        let _ = self.media_stream_source.RemoveStarting(self.starting);
        let _ = self.media_stream_source.RemoveSampleRequested(self.sample_requested);
    }
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for VideoEncoder {}
