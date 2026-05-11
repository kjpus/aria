use std::{
    ffi::OsStr,
    fs::File,
    io::ErrorKind,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use symphonia::{
    core::{
        audio::{AudioBufferRef, Layout, SampleBuffer},
        codecs::{CodecParameters, Decoder as SymphoniaDecoder, DecoderOptions},
        errors::Error as SymphoniaError,
        formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
        io::MediaSourceStream,
        meta::MetadataOptions,
        probe::Hint,
        sample::SampleFormat,
        units::Time,
    },
    default::{get_codecs, get_probe},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{self, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Media::{Audio, KernelStreaming, Multimedia},
        System::{Com, Threading},
    },
};

use super::PlaybackError;

const COMMAND_POLL_MS: u32 = 20;

pub struct ExclusivePlayer {
    state: Arc<ExclusiveState>,
    commands: Sender<ExclusiveCommand>,
    thread: Option<JoinHandle<()>>,
}

struct ExclusiveState {
    paused: AtomicBool,
    finished: AtomicBool,
    position_ms: AtomicU64,
    volume_bits: AtomicU32,
}

enum ExclusiveCommand {
    Play,
    Pause,
    Terminate,
}

struct ExclusiveInit {
    decoder: ExclusiveDecoder,
    audio_client: Audio::IAudioClient,
    render_client: Audio::IAudioRenderClient,
    render_event: Foundation::HANDLE,
    buffer_frames: u32,
    output_format: WasapiPcmFormat,
}

struct MmcssRegistration {
    handle: Foundation::HANDLE,
}

struct ExclusiveDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymphoniaDecoder>,
    track_id: u32,
    stream_spec: DecodedStreamSpec,
    pending_bytes: Vec<u8>,
    pending_cursor: usize,
    exhausted: bool,
}

#[derive(Clone, Copy)]
struct DecodedStreamSpec {
    channels: u16,
    sample_rate: u32,
    channel_mask: u32,
    preferred_kind: PreferredPcmKind,
    preferred_valid_bits: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreferredPcmKind {
    Integer,
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasapiSampleKind {
    I16,
    I32,
    F32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WasapiPcmFormat {
    kind: WasapiSampleKind,
    sample_rate: u32,
    channels: u16,
    channel_mask: u32,
    valid_bits: u16,
}

pub fn is_wasapi_device_id(device_id: &str) -> bool {
    wasapi_endpoint_id(device_id).is_some()
}

impl ExclusivePlayer {
    pub fn open(
        device_id: &str,
        path: &str,
        start_position: Duration,
        start_paused: bool,
        volume: f32,
    ) -> Result<Self, PlaybackError> {
        let endpoint_id = wasapi_endpoint_id(device_id).ok_or_else(|| {
            PlaybackError::Output(format!("unsupported Windows device id: {device_id}"))
        })?;
        let state = Arc::new(ExclusiveState {
            paused: AtomicBool::new(start_paused),
            finished: AtomicBool::new(false),
            position_ms: AtomicU64::new(start_position.as_millis() as u64),
            volume_bits: AtomicU32::new(normalize_volume(volume).to_bits()),
        });
        let (command_tx, command_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let state_for_thread = state.clone();
        let endpoint_id = endpoint_id.to_string();
        let path = path.to_string();

        let thread = thread::Builder::new()
            .name("aria-wasapi-exclusive".into())
            .spawn(move || {
                run_exclusive_thread(
                    endpoint_id,
                    path,
                    start_position,
                    start_paused,
                    state_for_thread,
                    command_rx,
                    init_tx,
                );
            })
            .map_err(|error| PlaybackError::Output(error.to_string()))?;

        match init_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                state,
                commands: command_tx,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                Err(PlaybackError::Output(error.to_string()))
            }
        }
    }

    pub fn play(&self) {
        let _ = self.commands.send(ExclusiveCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(ExclusiveCommand::Pause);
    }

    pub fn is_paused(&self) -> bool {
        self.state.paused.load(Ordering::Relaxed)
    }

    pub fn empty(&self) -> bool {
        self.state.finished.load(Ordering::Relaxed)
    }

    pub fn get_pos(&self) -> Duration {
        Duration::from_millis(self.state.position_ms.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, volume: f32) {
        self.state
            .volume_bits
            .store(normalize_volume(volume).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for ExclusivePlayer {
    fn drop(&mut self) {
        let _ = self.commands.send(ExclusiveCommand::Terminate);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl ExclusiveDecoder {
    fn open(path: &str, start_position: Duration) -> Result<Self, PlaybackError> {
        let file = File::open(path).map_err(|source| PlaybackError::OpenTrack {
            path: path.to_string(),
            source,
        })?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = Path::new(path).extension().and_then(|value| value.to_str()) {
            if !extension.is_empty() {
                hint.with_extension(extension);
            }
        }

        let probed = get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
        let mut format = probed.format;
        let track = format
            .default_track()
            .cloned()
            .ok_or_else(|| PlaybackError::DecodeTrack {
                path: path.to_string(),
            })?;
        let mut decoder = get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|error| PlaybackError::Output(error.to_string()))?;

        if start_position > Duration::ZERO {
            let _ = format.seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: Time::from(start_position.as_secs_f64()),
                    track_id: Some(track.id),
                },
            );
            decoder.reset();
        }

        let stream_spec = decoded_stream_spec(&track.codec_params).ok_or_else(|| {
            PlaybackError::Output(format!(
                "Unable to determine the decoded stream format for exclusive playback: {path}"
            ))
        })?;

        Ok(Self {
            format,
            decoder,
            track_id: track.id,
            stream_spec,
            pending_bytes: Vec::new(),
            pending_cursor: 0,
            exhausted: false,
        })
    }

    fn stream_spec(&self) -> DecodedStreamSpec {
        self.stream_spec
    }

    fn is_exhausted(&self) -> bool {
        self.exhausted && self.pending_cursor >= self.pending_bytes.len()
    }

    fn copy_next_bytes_into(
        &mut self,
        destination: &mut [u8],
        output_format: WasapiPcmFormat,
        volume: f32,
    ) -> Result<usize, PlaybackError> {
        let mut copied = 0usize;

        while copied < destination.len() {
            if self.pending_cursor >= self.pending_bytes.len() {
                if self.exhausted {
                    break;
                }

                self.refill_pending_bytes(output_format, volume)?;
                if self.pending_cursor >= self.pending_bytes.len() {
                    break;
                }
            }

            let remaining = self.pending_bytes.len() - self.pending_cursor;
            let copy_len = remaining.min(destination.len() - copied);
            destination[copied..copied + copy_len].copy_from_slice(
                &self.pending_bytes[self.pending_cursor..self.pending_cursor + copy_len],
            );
            self.pending_cursor += copy_len;
            copied += copy_len;
        }

        Ok(copied)
    }

    fn refill_pending_bytes(
        &mut self,
        output_format: WasapiPcmFormat,
        volume: f32,
    ) -> Result<(), PlaybackError> {
        self.pending_bytes.clear();
        self.pending_cursor = 0;

        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    self.exhausted = true;
                    return Ok(());
                }
                Err(SymphoniaError::ResetRequired) => {
                    return Err(PlaybackError::Output(
                        "Symphonia requested a decoder reset during exclusive playback".into(),
                    ));
                }
                Err(error) => return Err(PlaybackError::Output(error.to_string())),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(audio_buffer) => {
                    write_audio_buffer_bytes(
                        &mut self.pending_bytes,
                        audio_buffer,
                        output_format,
                        volume,
                    )?;
                    if self.pending_bytes.is_empty() {
                        continue;
                    }
                    return Ok(());
                }
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                    self.exhausted = true;
                    return Ok(());
                }
                Err(SymphoniaError::ResetRequired) => {
                    return Err(PlaybackError::Output(
                        "Symphonia requested a decoder reset during exclusive playback".into(),
                    ));
                }
                Err(error) => return Err(PlaybackError::Output(error.to_string())),
            }
        }
    }
}

impl WasapiPcmFormat {
    fn container_bits(self) -> u16 {
        match self.kind {
            WasapiSampleKind::I16 => 16,
            WasapiSampleKind::I32 | WasapiSampleKind::F32 => 32,
        }
    }

    fn bytes_per_sample(self) -> u16 {
        self.container_bits() / 8
    }

    fn block_align(self) -> u16 {
        self.channels * self.bytes_per_sample()
    }

    fn subtype(self) -> windows::core::GUID {
        match self.kind {
            WasapiSampleKind::F32 => Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
            WasapiSampleKind::I16 | WasapiSampleKind::I32 => {
                KernelStreaming::KSDATAFORMAT_SUBTYPE_PCM
            }
        }
    }

    fn display_label(self) -> String {
        match self.kind {
            WasapiSampleKind::F32 => {
                format!("{} Hz / {} ch float32", self.sample_rate, self.channels)
            }
            WasapiSampleKind::I16 => {
                format!("{} Hz / {} ch PCM 16-bit", self.sample_rate, self.channels)
            }
            WasapiSampleKind::I32 => format!(
                "{} Hz / {} ch PCM {}-in-32",
                self.sample_rate, self.channels, self.valid_bits
            ),
        }
    }
}

fn run_exclusive_thread(
    endpoint_id: String,
    path: String,
    start_position: Duration,
    start_paused: bool,
    state: Arc<ExclusiveState>,
    commands: Receiver<ExclusiveCommand>,
    init_tx: Sender<Result<(), PlaybackError>>,
) {
    let com_initialized = unsafe { Com::CoInitializeEx(None, Com::COINIT_MULTITHREADED).is_ok() };
    let _mmcss = register_current_thread_for_audio();

    let init = initialize_exclusive_output(&endpoint_id, &path, start_position);
    let mut init = match init {
        Ok(init) => {
            let _ = init_tx.send(Ok(()));
            init
        }
        Err(error) => {
            let _ = init_tx.send(Err(error));
            if com_initialized {
                unsafe {
                    Com::CoUninitialize();
                }
            }
            return;
        }
    };

    let mut playing = !start_paused;
    let mut source_finished = false;
    let mut frames_rendered = position_to_frames(start_position, init.output_format.sample_rate);

    if !source_finished {
        match fill_render_buffer(
            &init.render_client,
            init.buffer_frames,
            init.output_format,
            &mut init.decoder,
            &state,
            &mut frames_rendered,
        ) {
            Ok(finished) => source_finished = finished,
            Err(error) => {
                eprintln!("exclusive playback prefill failed: {error}");
                state.finished.store(true, Ordering::Relaxed);
                cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
                return;
            }
        }
    }

    if playing {
        if let Err(error) = unsafe { init.audio_client.Start() } {
            eprintln!("exclusive playback start failed: {error}");
            state.finished.store(true, Ordering::Relaxed);
            cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
            return;
        }
    }

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                ExclusiveCommand::Play => {
                    if !playing {
                        if let Err(error) = unsafe { init.audio_client.Start() } {
                            eprintln!("exclusive playback resume failed: {error}");
                            state.finished.store(true, Ordering::Relaxed);
                            cleanup_and_uninitialize(
                                com_initialized,
                                init.audio_client,
                                init.render_event,
                            );
                            return;
                        }
                        playing = true;
                        state.paused.store(false, Ordering::Relaxed);
                    }
                }
                ExclusiveCommand::Pause => {
                    if playing {
                        if let Err(error) = unsafe { init.audio_client.Stop() } {
                            eprintln!("exclusive playback pause failed: {error}");
                        }
                        playing = false;
                        state.paused.store(true, Ordering::Relaxed);
                    }
                }
                ExclusiveCommand::Terminate => {
                    state.finished.store(true, Ordering::Relaxed);
                    cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
                    return;
                }
            }
        }

        if source_finished {
            match current_padding(&init.audio_client) {
                Ok(0) => {
                    state.finished.store(true, Ordering::Relaxed);
                    cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
                    return;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("exclusive playback padding check failed: {error}");
                    state.finished.store(true, Ordering::Relaxed);
                    cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
                    return;
                }
            }
        }

        let wait_result =
            unsafe { Threading::WaitForSingleObject(init.render_event, COMMAND_POLL_MS) };

        match wait_result {
            WAIT_OBJECT_0 if playing && !source_finished => {
                match fill_render_buffer(
                    &init.render_client,
                    init.buffer_frames,
                    init.output_format,
                    &mut init.decoder,
                    &state,
                    &mut frames_rendered,
                ) {
                    Ok(finished) => source_finished = finished,
                    Err(error) => {
                        eprintln!("exclusive playback render failed: {error}");
                        state.finished.store(true, Ordering::Relaxed);
                        cleanup_and_uninitialize(
                            com_initialized,
                            init.audio_client,
                            init.render_event,
                        );
                        return;
                    }
                }
            }
            WAIT_OBJECT_0 | WAIT_TIMEOUT => {}
            WAIT_FAILED => {
                let error = unsafe { Foundation::GetLastError() };
                eprintln!("exclusive playback wait failed: {error:?}");
                state.finished.store(true, Ordering::Relaxed);
                cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
                return;
            }
            _ => {}
        }
    }
}

fn initialize_exclusive_output(
    endpoint_id: &str,
    path: &str,
    start_position: Duration,
) -> Result<ExclusiveInit, PlaybackError> {
    let mut decoder = ExclusiveDecoder::open(path, start_position)?;
    let stream_spec = decoder.stream_spec();

    let enumerator = unsafe {
        Com::CoCreateInstance::<_, Audio::IMMDeviceEnumerator>(
            &Audio::MMDeviceEnumerator,
            None,
            Com::CLSCTX_ALL,
        )
        .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    let endpoint_wide = wide_null(endpoint_id);
    let endpoint_pcwstr = PCWSTR(endpoint_wide.as_ptr());
    let device = unsafe {
        enumerator
            .GetDevice(endpoint_pcwstr)
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    let mut audio_client = activate_audio_client(&device)?;

    let output_format = choose_supported_format(&audio_client, stream_spec).ok_or_else(|| {
        PlaybackError::Output(format!(
            "WASAPI exclusive does not support a compatible PCM layout for {} Hz / {} ch",
            stream_spec.sample_rate, stream_spec.channels
        ))
    })?;

    let wave_format = build_wave_format(output_format);

    let mut default_period = 0_i64;
    let mut minimum_period = 0_i64;
    unsafe {
        audio_client
            .GetDevicePeriod(Some(&mut default_period), Some(&mut minimum_period))
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
    }
    let requested_period = if minimum_period > 0 {
        minimum_period
    } else {
        default_period
    };

    let render_event = unsafe {
        Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    initialize_audio_client_exclusive(
        &device,
        &mut audio_client,
        requested_period,
        output_format,
        &wave_format,
    )?;

    unsafe {
        audio_client
            .SetEventHandle(render_event)
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
    }

    let buffer_frames = unsafe {
        audio_client
            .GetBufferSize()
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    let render_client = unsafe {
        audio_client
            .GetService::<Audio::IAudioRenderClient>()
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    decoder
        .pending_bytes
        .reserve(buffer_frames as usize * output_format.block_align() as usize);

    Ok(ExclusiveInit {
        decoder,
        audio_client,
        render_client,
        render_event,
        buffer_frames,
        output_format,
    })
}

fn activate_audio_client(device: &Audio::IMMDevice) -> Result<Audio::IAudioClient, PlaybackError> {
    unsafe {
        device
            .Activate(Com::CLSCTX_ALL, None)
            .map_err(|error| PlaybackError::Output(error.to_string()))
    }
}

fn initialize_audio_client_exclusive(
    device: &Audio::IMMDevice,
    audio_client: &mut Audio::IAudioClient,
    requested_period: i64,
    output_format: WasapiPcmFormat,
    wave_format: &Audio::WAVEFORMATEXTENSIBLE,
) -> Result<(), PlaybackError> {
    let initialize = |client: &Audio::IAudioClient, period: i64| unsafe {
        client.Initialize(
            Audio::AUDCLNT_SHAREMODE_EXCLUSIVE,
            Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            period,
            period,
            &wave_format.Format,
            None,
        )
    };

    match initialize(audio_client, requested_period) {
        Ok(()) => Ok(()),
        Err(error) if error.code() == Audio::AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
            let aligned_frames = unsafe {
                audio_client
                    .GetBufferSize()
                    .map_err(|buffer_error| PlaybackError::Output(buffer_error.to_string()))?
            };
            let aligned_period =
                frames_to_reference_time(aligned_frames, output_format.sample_rate);

            *audio_client = activate_audio_client(device)?;
            initialize(audio_client, aligned_period)
                .map_err(|retry_error| PlaybackError::Output(retry_error.to_string()))
        }
        Err(error) => Err(PlaybackError::Output(error.to_string())),
    }
}

fn fill_render_buffer(
    render_client: &Audio::IAudioRenderClient,
    frames_available: u32,
    output_format: WasapiPcmFormat,
    decoder: &mut ExclusiveDecoder,
    state: &ExclusiveState,
    frames_rendered: &mut u64,
) -> Result<bool, PlaybackError> {
    let byte_count = frames_available as usize * output_format.block_align() as usize;
    let buffer = unsafe {
        render_client
            .GetBuffer(frames_available)
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };
    let bytes = unsafe { std::slice::from_raw_parts_mut(buffer as *mut u8, byte_count) };

    let bytes_written =
        decoder.copy_next_bytes_into(bytes, output_format, current_volume(state))?;
    if bytes_written < byte_count {
        bytes[bytes_written..].fill(0);
    }

    unsafe {
        render_client
            .ReleaseBuffer(frames_available, 0)
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
    }

    let frames_written_now = bytes_written / output_format.block_align() as usize;
    *frames_rendered += frames_written_now as u64;
    state.position_ms.store(
        frames_to_millis(*frames_rendered, output_format.sample_rate),
        Ordering::Relaxed,
    );

    Ok(bytes_written < byte_count && decoder.is_exhausted())
}

fn decoded_stream_spec(codec_params: &CodecParameters) -> Option<DecodedStreamSpec> {
    let sample_rate = codec_params.sample_rate?;
    let channels = codec_params
        .channels
        .or_else(|| codec_params.channel_layout.map(Layout::into_channels))?;
    let channel_count = channels.count() as u16;
    if channel_count == 0 {
        return None;
    }

    let (preferred_kind, default_valid_bits) = match codec_params.sample_format {
        Some(SampleFormat::F32 | SampleFormat::F64) => (PreferredPcmKind::Float, 32),
        Some(SampleFormat::S24 | SampleFormat::U24) => (PreferredPcmKind::Integer, 24),
        Some(SampleFormat::S32 | SampleFormat::U32) => (PreferredPcmKind::Integer, 32),
        Some(SampleFormat::S16 | SampleFormat::U16) => (PreferredPcmKind::Integer, 16),
        Some(SampleFormat::S8 | SampleFormat::U8) => (PreferredPcmKind::Integer, 16),
        None => {
            let inferred_bits = codec_params
                .bits_per_sample
                .or(codec_params.bits_per_coded_sample)
                .unwrap_or(16) as u16;
            if inferred_bits > 24 {
                (PreferredPcmKind::Integer, 32)
            } else if inferred_bits > 16 {
                (PreferredPcmKind::Integer, 24)
            } else {
                (PreferredPcmKind::Integer, 16)
            }
        }
    };

    let preferred_valid_bits = codec_params
        .bits_per_sample
        .or(codec_params.bits_per_coded_sample)
        .map(|bits| bits as u16)
        .unwrap_or(default_valid_bits);

    Some(DecodedStreamSpec {
        channels: channel_count,
        sample_rate,
        channel_mask: channels.bits(),
        preferred_kind,
        preferred_valid_bits,
    })
}

fn choose_supported_format(
    audio_client: &Audio::IAudioClient,
    stream_spec: DecodedStreamSpec,
) -> Option<WasapiPcmFormat> {
    for candidate in format_candidates(stream_spec) {
        let wave_format = build_wave_format(candidate);
        let result = unsafe {
            audio_client.IsFormatSupported(
                Audio::AUDCLNT_SHAREMODE_EXCLUSIVE,
                &wave_format.Format,
                None,
            )
        };

        if result.is_ok() {
            return Some(candidate);
        }
    }

    None
}

fn format_candidates(stream_spec: DecodedStreamSpec) -> Vec<WasapiPcmFormat> {
    let base = |kind, valid_bits| WasapiPcmFormat {
        kind,
        sample_rate: stream_spec.sample_rate,
        channels: stream_spec.channels,
        channel_mask: stream_spec.channel_mask,
        valid_bits,
    };

    let mut candidates = Vec::new();
    match stream_spec.preferred_kind {
        PreferredPcmKind::Float => {
            candidates.push(base(WasapiSampleKind::F32, 32));
            candidates.push(base(WasapiSampleKind::I32, 32));
            candidates.push(base(WasapiSampleKind::I32, 24));
            candidates.push(base(WasapiSampleKind::I16, 16));
        }
        PreferredPcmKind::Integer if stream_spec.preferred_valid_bits <= 16 => {
            candidates.push(base(WasapiSampleKind::I16, 16));
            candidates.push(base(WasapiSampleKind::I32, 16));
            candidates.push(base(WasapiSampleKind::I32, 32));
            candidates.push(base(WasapiSampleKind::F32, 32));
        }
        PreferredPcmKind::Integer if stream_spec.preferred_valid_bits <= 24 => {
            candidates.push(base(WasapiSampleKind::I32, 24));
            candidates.push(base(WasapiSampleKind::I32, 32));
            candidates.push(base(WasapiSampleKind::F32, 32));
            candidates.push(base(WasapiSampleKind::I16, 16));
        }
        PreferredPcmKind::Integer => {
            candidates.push(base(WasapiSampleKind::I32, 32));
            candidates.push(base(WasapiSampleKind::F32, 32));
            candidates.push(base(WasapiSampleKind::I32, 24));
            candidates.push(base(WasapiSampleKind::I16, 16));
        }
    }

    let mut deduped = Vec::new();
    for candidate in candidates {
        if !deduped.contains(&candidate) {
            deduped.push(candidate);
        }
    }
    deduped
}

fn write_audio_buffer_bytes(
    destination: &mut Vec<u8>,
    audio_buffer: AudioBufferRef<'_>,
    output_format: WasapiPcmFormat,
    volume: f32,
) -> Result<(), PlaybackError> {
    destination.clear();

    match output_format.kind {
        WasapiSampleKind::I16 => {
            let mut buffer =
                SampleBuffer::<i16>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            apply_i16_volume(buffer.samples_mut(), volume);
            destination.extend_from_slice(as_bytes(buffer.samples()));
        }
        WasapiSampleKind::I32 => {
            let mut buffer =
                SampleBuffer::<i32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            apply_i32_volume(buffer.samples_mut(), volume);
            destination.extend_from_slice(as_bytes(buffer.samples()));
        }
        WasapiSampleKind::F32 => {
            let mut buffer =
                SampleBuffer::<f32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            apply_f32_volume(buffer.samples_mut(), volume);
            destination.extend_from_slice(as_bytes(buffer.samples()));
        }
    }

    if destination.len() % output_format.block_align() as usize != 0 {
        return Err(PlaybackError::Output(format!(
            "decoded packet does not align to WASAPI block size for {}",
            output_format.display_label()
        )));
    }

    Ok(())
}

fn current_padding(audio_client: &Audio::IAudioClient) -> Result<u32, PlaybackError> {
    unsafe {
        audio_client
            .GetCurrentPadding()
            .map_err(|error| PlaybackError::Output(error.to_string()))
    }
}

fn cleanup_and_uninitialize(
    com_initialized: bool,
    audio_client: Audio::IAudioClient,
    render_event: Foundation::HANDLE,
) {
    let _ = unsafe { audio_client.Stop() };
    let _ = unsafe { Foundation::CloseHandle(render_event) };

    if com_initialized {
        unsafe {
            Com::CoUninitialize();
        }
    }
}

impl Drop for MmcssRegistration {
    fn drop(&mut self) {
        let _ = unsafe { Threading::AvRevertMmThreadCharacteristics(self.handle) };
    }
}

fn register_current_thread_for_audio() -> Option<MmcssRegistration> {
    let mut task_index = 0u32;
    let task_name = wide_null("Pro Audio");
    let handle = match unsafe {
        Threading::AvSetMmThreadCharacteristicsW(PCWSTR(task_name.as_ptr()), &mut task_index)
    } {
        Ok(handle) => handle,
        Err(error) => {
            eprintln!("failed to register exclusive playback thread with MMCSS: {error}");
            return None;
        }
    };

    if let Err(error) = unsafe { Threading::AvSetMmThreadPriority(handle, Threading::AVRT_PRIORITY_HIGH) } {
        eprintln!("failed to raise exclusive playback MMCSS priority: {error}");
        let _ = unsafe { Threading::AvRevertMmThreadCharacteristics(handle) };
        return None;
    }

    Some(MmcssRegistration { handle })
}

fn wasapi_endpoint_id(device_id: &str) -> Option<&str> {
    device_id.strip_prefix("wasapi:")
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn position_to_frames(position: Duration, sample_rate: u32) -> u64 {
    (position.as_secs_f64() * sample_rate as f64).round() as u64
}

fn frames_to_millis(frames: u64, sample_rate: u32) -> u64 {
    ((frames as f64 / sample_rate as f64) * 1000.0).round() as u64
}

fn frames_to_reference_time(frames: u32, sample_rate: u32) -> i64 {
    let numerator = frames as u64 * 10_000_000u64;
    let denominator = sample_rate.max(1) as u64;
    numerator.div_ceil(denominator).max(1) as i64
}

fn build_wave_format(output_format: WasapiPcmFormat) -> Audio::WAVEFORMATEXTENSIBLE {
    let bits_per_sample = output_format.container_bits();
    let block_align = output_format.block_align();
    let cb_size = (std::mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>()
        - std::mem::size_of::<Audio::WAVEFORMATEX>()) as u16;
    let channel_mask = if output_format.channel_mask == 0 {
        KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT
    } else {
        output_format.channel_mask
    };

    Audio::WAVEFORMATEXTENSIBLE {
        Format: Audio::WAVEFORMATEX {
            wFormatTag: KernelStreaming::WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: output_format.channels,
            nSamplesPerSec: output_format.sample_rate,
            nAvgBytesPerSec: output_format.sample_rate * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: bits_per_sample,
            cbSize: cb_size,
        },
        Samples: Audio::WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: output_format.valid_bits,
        },
        dwChannelMask: channel_mask,
        SubFormat: output_format.subtype(),
    }
}

fn as_bytes<T>(samples: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            samples.as_ptr() as *const u8,
            std::mem::size_of_val(samples),
        )
    }
}

fn current_volume(state: &ExclusiveState) -> f32 {
    f32::from_bits(state.volume_bits.load(Ordering::Relaxed))
}

fn normalize_volume(volume: f32) -> f32 {
    if !volume.is_finite() {
        return 1.0;
    }

    volume.clamp(0.0, 1.0)
}

fn apply_i16_volume(samples: &mut [i16], volume: f32) {
    if volume >= 0.999_9 {
        return;
    }

    if volume <= 0.000_1 {
        samples.fill(0);
        return;
    }

    for sample in samples {
        let scaled = (*sample as f32 * volume)
            .round()
            .clamp(i16::MIN as f32, i16::MAX as f32);
        *sample = scaled as i16;
    }
}

fn apply_i32_volume(samples: &mut [i32], volume: f32) {
    if volume >= 0.999_9 {
        return;
    }

    if volume <= 0.000_1 {
        samples.fill(0);
        return;
    }

    let factor = volume as f64;
    for sample in samples {
        let scaled = (*sample as f64 * factor)
            .round()
            .clamp(i32::MIN as f64, i32::MAX as f64);
        *sample = scaled as i32;
    }
}

fn apply_f32_volume(samples: &mut [f32], volume: f32) {
    if volume >= 0.999_9 {
        return;
    }

    if volume <= 0.000_1 {
        samples.fill(0.0);
        return;
    }

    for sample in samples {
        *sample = (*sample * volume).clamp(-1.0, 1.0);
    }
}
