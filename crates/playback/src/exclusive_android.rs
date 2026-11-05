use std::{
    fs::File,
    io::ErrorKind,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, OnceLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use jni::{
    objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue},
    AttachGuard, JavaVM,
};

static GLOBAL_VM: OnceLock<JavaVM> = OnceLock::new();
static GLOBAL_APP_CONTEXT: OnceLock<GlobalRef> = OnceLock::new();
static GLOBAL_CLASS_LOADER: OnceLock<GlobalRef> = OnceLock::new();

pub fn register_android_context(env: &mut jni::JNIEnv, context: &JObject) {
    if let Ok(vm) = env.get_java_vm() {
        let _ = GLOBAL_VM.set(vm);
    }
    if !context.is_null() {
        let app_ctx = match env.call_method(context, "getApplicationContext", "()Landroid/content/Context;", &[]) {
            Ok(val) => match val.l() {
                Ok(obj) if !obj.is_null() => Some(obj),
                _ => None,
            },
            Err(_) => {
                let _ = env.exception_clear();
                None
            }
        };
        let target: &JObject = match &app_ctx {
            Some(obj) => obj,
            None => context,
        };
        if GLOBAL_APP_CONTEXT.get().is_none() {
            if let Ok(g) = env.new_global_ref(target) {
                let _ = GLOBAL_APP_CONTEXT.set(g);
            }
        }
        if GLOBAL_CLASS_LOADER.get().is_none() {
            if let Some(ctx_ref) = GLOBAL_APP_CONTEXT.get() {
                let target_obj = unsafe { JObject::from_raw(ctx_ref.as_raw()) };
                if let Ok(cl_val) = env.call_method(&target_obj, "getClassLoader", "()Ljava/lang/ClassLoader;", &[]) {
                    if let Ok(cl_obj) = cl_val.l() {
                        if !cl_obj.is_null() {
                            if let Ok(cl_g) = env.new_global_ref(&cl_obj) {
                                let _ = GLOBAL_CLASS_LOADER.set(cl_g);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn get_android_jni() -> Option<(AttachGuard<'static>, JObject<'static>, JObject<'static>)> {
    let vm = GLOBAL_VM.get()?;
    let ctx_ref = GLOBAL_APP_CONTEXT.get()?;
    let env = vm.attach_current_thread().ok()?;
    let app_context = unsafe { JObject::from_raw(ctx_ref.as_raw()) };
    let class_loader = if let Some(cl_ref) = GLOBAL_CLASS_LOADER.get() {
        unsafe { JObject::from_raw(cl_ref.as_raw()) }
    } else {
        JObject::null()
    };
    Some((env, app_context, class_loader))
}
use symphonia::core::{
    audio::{AudioBufferRef, Layout, SampleBuffer},
    codecs::{CodecParameters, Decoder as SymphoniaDecoder, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
    sample::SampleFormat,
    units::Time,
};
use symphonia::default::{get_codecs, get_probe};

use super::PlaybackError;

const LOOKAHEAD_QUEUE_CAPACITY: usize = 64;

pub struct ExclusivePlayer {
    state: Arc<ExclusiveState>,
    commands: Sender<ExclusiveCommand>,
    thread: Option<JoinHandle<()>>,
    stream_spec: DecodedStreamSpec,
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

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct DecodedStreamSpec {
    channels: u16,
    sample_rate: u32,
    preferred_kind: PreferredPcmKind,
    preferred_valid_bits: u16,
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PreferredPcmKind {
    Integer,
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePcmFormat {
    Pcm16 = 1,
    Pcm24Packed = 2,
    Pcm32 = 3,
    PcmFloat = 4,
}

impl TryFrom<i32> for ActivePcmFormat {
    type Error = ();
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ActivePcmFormat::Pcm16),
            2 => Ok(ActivePcmFormat::Pcm24Packed),
            3 => Ok(ActivePcmFormat::Pcm32),
            4 => Ok(ActivePcmFormat::PcmFloat),
            _ => Err(()),
        }
    }
}

enum DecodedChunk {
    Data(Vec<u8>),
    EndOfStream,
}

struct ExclusiveDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn SymphoniaDecoder>,
    track_id: u32,
    stream_spec: DecodedStreamSpec,
    delay_frames: u32,
}

impl ExclusivePlayer {
    pub fn open(
        device_id: &str,
        path: &str,
        start_position: Duration,
        start_paused: bool,
        volume: f32,
    ) -> Result<Self, PlaybackError> {
        Self::open_internal(device_id, path, start_position, start_paused, volume, false)
    }

    pub fn open_prewarmed(
        device_id: &str,
        path: &str,
        volume: f32,
    ) -> Result<Self, PlaybackError> {
        Self::open_internal(device_id, path, Duration::ZERO, true, volume, true)
    }

    fn open_internal(
        device_id: &str,
        path: &str,
        start_position: Duration,
        start_paused: bool,
        volume: f32,
        is_prewarm: bool,
    ) -> Result<Self, PlaybackError> {
        let device_id_int: i32 = device_id.parse().unwrap_or(0);

        let decoder = ExclusiveDecoder::open(path, start_position)?;
        let stream_spec = decoder.stream_spec();

        let state = Arc::new(ExclusiveState {
            paused: AtomicBool::new(start_paused),
            finished: AtomicBool::new(false),
            position_ms: AtomicU64::new(start_position.as_millis() as u64),
            volume_bits: AtomicU32::new(normalize_volume(volume).to_bits()),
        });

        let (command_tx, command_rx) = mpsc::channel();
        let (init_tx, init_rx) = mpsc::channel();
        let state_for_thread = state.clone();
        let path = path.to_string();

        let thread = thread::Builder::new()
            .name("aria-android-bitperfect".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                run_android_exclusive_thread(
                    device_id_int,
                    decoder,
                    stream_spec,
                    start_paused,
                    is_prewarm,
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
                stream_spec,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(PlaybackError::Output(format!(
                "Failed to initialize Android bit-accurate playback for '{path}'"
            ))),
        }
    }

    pub fn audio_spec_string(&self) -> String {
        let bits = if self.stream_spec.preferred_valid_bits > 0 {
            self.stream_spec.preferred_valid_bits
        } else {
            16
        };
        let sample_rate_khz = self.stream_spec.sample_rate as f64 / 1000.0;
        if (sample_rate_khz.fract()).abs() < 0.001 {
            format!("{}-bit / {:.0} kHz", bits, sample_rate_khz)
        } else {
            format!("{}-bit / {:.1} kHz", bits, sample_rate_khz)
        }
    }

    pub fn play(&self) {
        self.state.paused.store(false, Ordering::Relaxed);
        let _ = self.commands.send(ExclusiveCommand::Play);
    }

    pub fn pause(&self) {
        self.state.paused.store(true, Ordering::Relaxed);
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
                "Unable to determine the decoded stream format for bit-accurate playback: {path}"
            ))
        })?;

        let delay_frames = if start_position == Duration::ZERO {
            track.codec_params.delay.unwrap_or(0)
        } else {
            0
        };

        Ok(Self {
            format,
            decoder,
            track_id: track.id,
            stream_spec,
            delay_frames,
        })
    }

    fn stream_spec(&self) -> DecodedStreamSpec {
        self.stream_spec
    }
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
        preferred_kind,
        preferred_valid_bits,
    })
}

fn spawn_decode_worker(
    mut decoder: ExclusiveDecoder,
    active_format: ActivePcmFormat,
    output_sample_rate: u32,
    stop_flag: Arc<AtomicBool>,
) -> (
    JoinHandle<()>,
    Receiver<Result<DecodedChunk, PlaybackError>>,
) {
    let (tx, rx) =
        mpsc::sync_channel::<Result<DecodedChunk, PlaybackError>>(LOOKAHEAD_QUEUE_CAPACITY);

    let handle = thread::Builder::new()
        .name("aria-android-decode".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            let mut pending_buffer = Vec::new();
            let mut remaining_delay = decoder.delay_frames;
            let source_spec = decoder.stream_spec();
            let mut resampler = crate::resample::AudioResampler::new(
                source_spec.sample_rate,
                output_sample_rate,
                source_spec.channels as usize,
            )
            .expect("failed to create audio resampler");
            let mut resampled_f32 = Vec::new();

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                let packet = match decoder.format.next_packet() {
                    Ok(packet) => packet,
                    Err(SymphoniaError::IoError(error))
                        if error.kind() == ErrorKind::UnexpectedEof =>
                    {
                        if resampler.is_resampling() {
                            resampled_f32.clear();
                            if let Ok(()) = resampler.flush(&mut resampled_f32) {
                                if !resampled_f32.is_empty() {
                                    write_f32_samples_to_android_bytes(
                                        &mut pending_buffer,
                                        &resampled_f32,
                                        active_format,
                                    );
                                    if !pending_buffer.is_empty() {
                                        let chunk = std::mem::take(&mut pending_buffer);
                                        let _ = tx.send(Ok(DecodedChunk::Data(chunk)));
                                    }
                                }
                            }
                        }
                        let _ = tx.send(Ok(DecodedChunk::EndOfStream));
                        break;
                    }
                    Err(SymphoniaError::ResetRequired) => {
                        let _ = tx.send(Err(PlaybackError::Output(
                            "Symphonia requested a decoder reset during bit-accurate playback".into(),
                        )));
                        break;
                    }
                    Err(error) => {
                        let _ = tx.send(Err(PlaybackError::Output(error.to_string())));
                        break;
                    }
                };

                if packet.track_id() != decoder.track_id {
                    continue;
                }

                match decoder.decoder.decode(&packet) {
                    Ok(audio_buffer) => {
                        let total_frames = audio_buffer.frames() as u32;
                        let skip = if remaining_delay > 0 {
                            if total_frames <= remaining_delay {
                                remaining_delay -= total_frames;
                                continue;
                            } else {
                                let s = remaining_delay as usize;
                                remaining_delay = 0;
                                s
                            }
                        } else {
                            0
                        };

                        if !resampler.is_resampling() {
                            write_audio_buffer_bytes_with_skip(
                                &mut pending_buffer,
                                audio_buffer,
                                active_format,
                                skip,
                            );
                        } else {
                            let mut sample_buf = SampleBuffer::<f32>::new(
                                audio_buffer.frames() as u64,
                                *audio_buffer.spec(),
                            );
                            sample_buf.copy_interleaved_ref(audio_buffer);
                            let samples = sample_buf.samples();
                            let skip_samples = skip * source_spec.channels as usize;
                            resampled_f32.clear();
                            if skip_samples < samples.len() {
                                if let Err(error) = resampler
                                    .process_interleaved(&samples[skip_samples..], &mut resampled_f32)
                                {
                                    let _ = tx.send(Err(error));
                                    break;
                                }
                                if !resampled_f32.is_empty() {
                                    write_f32_samples_to_android_bytes(
                                        &mut pending_buffer,
                                        &resampled_f32,
                                        active_format,
                                    );
                                }
                            }
                        }

                        if pending_buffer.is_empty() {
                            continue;
                        }

                        let chunk = std::mem::take(&mut pending_buffer);
                        if tx.send(Ok(DecodedChunk::Data(chunk))).is_err() {
                            break;
                        }
                    }
                    Err(SymphoniaError::DecodeError(warning)) => {
                        eprintln!("exclusive decoder warning: {warning}");
                        continue;
                    }
                    Err(SymphoniaError::IoError(error))
                        if error.kind() == ErrorKind::UnexpectedEof =>
                    {
                        if resampler.is_resampling() {
                            resampled_f32.clear();
                            if let Ok(()) = resampler.flush(&mut resampled_f32) {
                                if !resampled_f32.is_empty() {
                                    write_f32_samples_to_android_bytes(
                                        &mut pending_buffer,
                                        &resampled_f32,
                                        active_format,
                                    );
                                    if !pending_buffer.is_empty() {
                                        let chunk = std::mem::take(&mut pending_buffer);
                                        let _ = tx.send(Ok(DecodedChunk::Data(chunk)));
                                    }
                                }
                            }
                        }
                        let _ = tx.send(Ok(DecodedChunk::EndOfStream));
                        break;
                    }
                    Err(SymphoniaError::ResetRequired) => {
                        let _ = tx.send(Err(PlaybackError::Output(
                            "Symphonia requested a decoder reset during bit-accurate playback".into(),
                        )));
                        break;
                    }
                    Err(error) => {
                        let _ = tx.send(Err(PlaybackError::Output(error.to_string())));
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn exclusive decoder thread");

    (handle, rx)
}

fn write_audio_buffer_bytes(
    destination: &mut Vec<u8>,
    audio_buffer: AudioBufferRef,
    active_format: ActivePcmFormat,
) {
    write_audio_buffer_bytes_with_skip(destination, audio_buffer, active_format, 0);
}

fn write_audio_buffer_bytes_with_skip(
    destination: &mut Vec<u8>,
    audio_buffer: AudioBufferRef,
    active_format: ActivePcmFormat,
    skip_frames: usize,
) {
    let channels = audio_buffer.spec().channels.count();
    let skip_samples = skip_frames * channels;

    match active_format {
        ActivePcmFormat::PcmFloat => {
            let mut buffer =
                SampleBuffer::<f32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            let samples = buffer.samples();
            if skip_samples < samples.len() {
                destination.extend_from_slice(as_bytes(&samples[skip_samples..]));
            }
        }
        ActivePcmFormat::Pcm16 => {
            let mut buffer =
                SampleBuffer::<i16>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            let samples = buffer.samples();
            if skip_samples < samples.len() {
                destination.extend_from_slice(as_bytes(&samples[skip_samples..]));
            }
        }
        ActivePcmFormat::Pcm24Packed => {
            let mut buffer =
                SampleBuffer::<f32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            let samples = buffer.samples();
            if skip_samples < samples.len() {
                for &sample in &samples[skip_samples..] {
                    let clamped = (sample * 8388607.0).round().clamp(-8388608.0, 8388607.0) as i32;
                    let bytes = clamped.to_le_bytes();
                    destination.extend_from_slice(&bytes[0..3]);
                }
            }
        }
        ActivePcmFormat::Pcm32 => {
            let mut buffer =
                SampleBuffer::<i32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            let samples = buffer.samples();
            if skip_samples < samples.len() {
                destination.extend_from_slice(as_bytes(&samples[skip_samples..]));
            }
        }
    }
}

fn write_f32_samples_to_android_bytes(
    destination: &mut Vec<u8>,
    samples: &[f32],
    active_format: ActivePcmFormat,
) {
    match active_format {
        ActivePcmFormat::PcmFloat => {
            destination.extend_from_slice(as_bytes(samples));
        }
        ActivePcmFormat::Pcm16 => {
            for &sample in samples {
                let clamped = (sample * 32767.0).round().clamp(-32768.0, 32767.0) as i16;
                destination.extend_from_slice(&clamped.to_le_bytes());
            }
        }
        ActivePcmFormat::Pcm24Packed => {
            for &sample in samples {
                let clamped = (sample * 8388607.0).round().clamp(-8388608.0, 8388607.0) as i32;
                let bytes = clamped.to_le_bytes();
                destination.extend_from_slice(&bytes[0..3]);
            }
        }
        ActivePcmFormat::Pcm32 => {
            for &sample in samples {
                let clamped =
                    (sample * 2147483647.0).round().clamp(-2147483648.0, 2147483647.0) as i32;
                destination.extend_from_slice(&clamped.to_le_bytes());
            }
        }
    }
}

fn as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            slice.len() * std::mem::size_of::<T>(),
        )
    }
}

fn run_android_exclusive_thread(
    device_id: i32,
    decoder: ExclusiveDecoder,
    stream_spec: DecodedStreamSpec,
    start_paused: bool,
    is_prewarm: bool,
    state: Arc<ExclusiveState>,
    commands: Receiver<ExclusiveCommand>,
    init_tx: Sender<Result<(), PlaybackError>>,
) {
    let (mut env, app_context, class_loader) = match get_android_jni() {
        Some((e, a, c)) => (e, a, c),
        None => {
            let _ = init_tx.send(Err(PlaybackError::Output("Android JNI context not registered".into())));
            return;
        }
    };

    let class_name = match env.new_string("com.aria.player.AriaBitPerfectPlayer") {
        Ok(s) => s,
        Err(err) => {
            let _ = env.exception_clear();
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to allocate class name string: {err}"
            ))));
            return;
        }
    };

    let player_class_obj = match env.call_method(
        &class_loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[(&class_name).into()],
    ) {
        Ok(val) => match val.l() {
            Ok(obj) => obj,
            Err(err) => {
                let _ = env.exception_clear();
                let _ = init_tx.send(Err(PlaybackError::Output(format!(
                    "Failed to obtain Class object from loadClass: {err}"
                ))));
                return;
            }
        },
        Err(err) => {
            let _ = env.exception_clear();
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to loadClass('com.aria.player.AriaBitPerfectPlayer'): {err}"
            ))));
            return;
        }
    };

    let player_class: JClass = player_class_obj.into();

    let player_obj = match env.new_object(&player_class, "()V", &[]) {
        Ok(obj) => obj,
        Err(err) => {
            let _ = env.exception_clear();
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to instantiate AriaBitPerfectPlayer: {err}"
            ))));
            return;
        }
    };

    let bit_depth = stream_spec.preferred_valid_bits as i32;
    aria_domain::log_diagnostic(
        aria_domain::DiagnosticSubsystem::Playback,
        aria_domain::DiagnosticLogLevel::Info,
        format!(
            "Configuring AudioTrack: device_id={}, sample_rate={}Hz, bits={}, channels={}, is_prewarm={}",
            device_id, stream_spec.sample_rate, bit_depth, stream_spec.channels, is_prewarm
        ),
        None,
    );

    let format_result = env.call_method(
        &player_obj,
        "open",
        "(Landroid/content/Context;IIIIZ)I",
        &[
            (&app_context).into(),
            JValue::Int(device_id),
            JValue::Int(stream_spec.sample_rate as i32),
            JValue::Int(stream_spec.channels as i32),
            JValue::Int(bit_depth),
            JValue::Bool(is_prewarm as u8),
        ],
    ).or_else(|_| {
        let _ = env.exception_clear();
        env.call_method(
            &player_obj,
            "open",
            "(Landroid/content/Context;IIII)I",
            &[
                (&app_context).into(),
                JValue::Int(device_id),
                JValue::Int(stream_spec.sample_rate as i32),
                JValue::Int(stream_spec.channels as i32),
                JValue::Int(bit_depth),
            ],
        )
    });

    let format_code = match format_result {
        Ok(val) => val.i().unwrap_or(0),
        Err(err) => {
            let _ = env.exception_clear();
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Error calling open on AriaBitPerfectPlayer: {err}"
            ))));
            return;
        }
    };

    let active_format = match ActivePcmFormat::try_from(format_code) {
        Ok(fmt) => {
            aria_domain::log_diagnostic(
                aria_domain::DiagnosticSubsystem::Playback,
                aria_domain::DiagnosticLogLevel::Info,
                format!("AudioTrack initialized with format_code {format_code} ({fmt:?})"),
                None,
            );
            fmt
        }
        Err(_) => {
            aria_domain::log_diagnostic(
                aria_domain::DiagnosticSubsystem::Playback,
                aria_domain::DiagnosticLogLevel::Error,
                format!(
                    "Failed to open Android bit-accurate stream (sampleRate={}Hz, bits={}, ch={})",
                    stream_spec.sample_rate, bit_depth, stream_spec.channels
                ),
                None,
            );
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to open Android bit-accurate stream (sampleRate={}Hz, bits={}, ch={})",
                stream_spec.sample_rate, bit_depth, stream_spec.channels
            ))));
            return;
        }
    };

    // Pre-allocate / fetch reusable Java write buffer to eliminate heap churn and JNI leaks
    let mut current_buf_capacity = 65536;
    let write_buf_obj = match env.call_method(
        &player_obj,
        "getWriteBuffer",
        "(I)[B",
        &[JValue::Int(current_buf_capacity as i32)],
    ) {
        Ok(val) => match val.l() {
            Ok(obj) => obj,
            Err(e) => {
                let _ = init_tx.send(Err(PlaybackError::Output(format!(
                    "Failed to get write buffer object: {e}"
                ))));
                return;
            }
        },
        Err(e) => {
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to call getWriteBuffer: {e}"
            ))));
            return;
        }
    };
    let mut write_buf_global = match env.new_global_ref(&write_buf_obj) {
        Ok(g) => g,
        Err(e) => {
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to create global ref for write buffer: {e}"
            ))));
            return;
        }
    };

    let active_sample_rate = match env.call_method(&player_obj, "getSampleRate", "()I", &[]) {
        Ok(val) => val.i().unwrap_or(stream_spec.sample_rate as i32) as u32,
        Err(_) => {
            let _ = env.exception_clear();
            stream_spec.sample_rate
        }
    };

    let _ = init_tx.send(Ok(()));

    // Spawn the background decode worker
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (decode_thread, chunk_rx) = spawn_decode_worker(decoder, active_format, active_sample_rate, stop_flag.clone());
    let mut chunk_rx = Some(chunk_rx);

    let mut is_paused = start_paused;
    let mut reached_eos = false;
    let mut chunk_counter: u32 = 0;
    let mut playback_started = false;

    if is_prewarm {
        // Lookahead pre-buffering: read first decoded chunk and prime AudioTrack sink
        if let Some(ref rx) = chunk_rx {
            if let Ok(Ok(DecodedChunk::Data(mut first_chunk))) = rx.recv_timeout(Duration::from_millis(1500)) {
                let volume = current_volume(&state);
                if volume < 0.999_9 && !first_chunk.is_empty() {
                    apply_volume(&mut first_chunk, active_format, volume);
                }

                if first_chunk.len() > current_buf_capacity {
                    current_buf_capacity = first_chunk.len().next_power_of_two();
                    if let Ok(val) = env.call_method(
                        &player_obj,
                        "getWriteBuffer",
                        "(I)[B",
                        &[JValue::Int(current_buf_capacity as i32)],
                    ) {
                        if let Ok(obj) = val.l() {
                            if let Ok(new_g) = env.new_global_ref(&obj) {
                                write_buf_global = new_g;
                            }
                        }
                    }
                }

                let slice_i8: &[i8] = unsafe {
                    std::slice::from_raw_parts(first_chunk.as_ptr() as *const i8, first_chunk.len())
                };

                let _ = env.with_local_frame(16, |env| {
                    let j_arr = unsafe { JByteArray::from_raw(write_buf_global.as_raw()) };
                    env.set_byte_array_region(&j_arr, 0, slice_i8)?;
                    let res = env.call_method(
                        &player_obj,
                        "writeDirect",
                        "(I)I",
                        &[JValue::Int(first_chunk.len() as i32)],
                    )?;
                    res.i()
                });
                let _ = env.exception_clear();
                chunk_counter = 1;
                aria_domain::log_diagnostic(
                    aria_domain::DiagnosticSubsystem::Playback,
                    aria_domain::DiagnosticLogLevel::Info,
                    format!("Pre-warmed AudioTrack sink primed with {} bytes", first_chunk.len()),
                    None,
                );
            }
        }
    }

    while !reached_eos {
        // Handle pending commands non-blocking
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                ExclusiveCommand::Play => {
                    is_paused = false;
                    playback_started = true;
                    let _ = env.call_method(&player_obj, "play", "()Z", &[]);
                }
                ExclusiveCommand::Pause => {
                    is_paused = true;
                    let _ = env.call_method(&player_obj, "pause", "()Z", &[]);
                }
                ExclusiveCommand::Terminate => {
                    stop_flag.store(true, Ordering::Relaxed);
                    drop(chunk_rx.take());
                    let _ = env.call_method(&player_obj, "close", "()Z", &[]);
                    let _ = env.exception_clear();
                    let _ = decode_thread.join();
                    return;
                }
            }
        }

        if is_paused {
            // Block with timeout waiting for next command
            if let Ok(cmd) = commands.recv_timeout(Duration::from_millis(50)) {
                match cmd {
                    ExclusiveCommand::Play => {
                        is_paused = false;
                        playback_started = true;
                        let _ = env.call_method(&player_obj, "play", "()Z", &[]);
                    }
                    ExclusiveCommand::Pause => {}
                    ExclusiveCommand::Terminate => {
                        stop_flag.store(true, Ordering::Relaxed);
                        drop(chunk_rx.take());
                        let _ = env.call_method(&player_obj, "close", "()Z", &[]);
                        let _ = env.exception_clear();
                        let _ = decode_thread.join();
                        return;
                    }
                }
            }
            continue;
        }

        let rx = match chunk_rx.as_ref() {
            Some(rx) => rx,
            None => break,
        };

        // Receive next chunk
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(DecodedChunk::Data(mut data))) => {
                let volume = current_volume(&state);
                if volume < 0.999_9 && !data.is_empty() {
                    apply_volume(&mut data, active_format, volume);
                }

                if data.len() > current_buf_capacity {
                    current_buf_capacity = data.len().next_power_of_two();
                    if let Ok(val) = env.call_method(
                        &player_obj,
                        "getWriteBuffer",
                        "(I)[B",
                        &[JValue::Int(current_buf_capacity as i32)],
                    ) {
                        if let Ok(obj) = val.l() {
                            if let Ok(new_g) = env.new_global_ref(&obj) {
                                write_buf_global = new_g;
                            }
                        }
                    }
                }

                // Copy data to the reusable pinned Java byte array and write directly
                let slice_i8: &[i8] = unsafe {
                    std::slice::from_raw_parts(data.as_ptr() as *const i8, data.len())
                };

                let write_res: Result<i32, jni::errors::Error> = env.with_local_frame(16, |env| {
                    let j_arr = unsafe { JByteArray::from_raw(write_buf_global.as_raw()) };
                    env.set_byte_array_region(&j_arr, 0, slice_i8)?;
                    let res = env.call_method(
                        &player_obj,
                        "writeDirect",
                        "(I)I",
                        &[JValue::Int(data.len() as i32)],
                    )?;
                    res.i()
                });
                let _ = env.exception_clear();

                match write_res {
                    Ok(written) if written < 0 => {
                        aria_domain::log_diagnostic(
                            aria_domain::DiagnosticSubsystem::Playback,
                            aria_domain::DiagnosticLogLevel::Error,
                            format!("AudioTrack writeDirect returned error code {written}"),
                            None,
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        aria_domain::log_diagnostic(
                            aria_domain::DiagnosticSubsystem::Playback,
                            aria_domain::DiagnosticLogLevel::Error,
                            format!("Failed to invoke writeDirect: {e}"),
                            None,
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    _ => {}
                }

                // Pre-buffering: start playback once the first chunk is written into AudioTrack buffer
                if !playback_started && !is_paused {
                    playback_started = true;
                    let _ = env.call_method(&player_obj, "play", "()Z", &[]);
                    aria_domain::log_diagnostic(
                        aria_domain::DiagnosticSubsystem::Playback,
                        aria_domain::DiagnosticLogLevel::Info,
                        "AudioTrack play initiated with pre-buffered audio in sink",
                        None,
                    );
                }

                // Update position throttled (every ~150ms instead of every chunk)
                chunk_counter = chunk_counter.wrapping_add(1);
                if chunk_counter % 16 == 0 {
                    let _: Result<(), jni::errors::Error> = env.with_local_frame(16, |env| {
                        if let Ok(pos_val) = env.call_method(&player_obj, "getPositionMs", "()J", &[]) {
                            if let Ok(pos_ms) = pos_val.j() {
                                if pos_ms >= 0 {
                                    state.position_ms.store(pos_ms as u64, Ordering::Relaxed);
                                }
                            }
                        }
                        Ok(())
                    });
                    let _ = env.exception_clear();
                }

                if chunk_counter % 256 == 0 {
                    aria_domain::log_diagnostic(
                        aria_domain::DiagnosticSubsystem::Playback,
                        aria_domain::DiagnosticLogLevel::Debug,
                        format!("Playback active: chunk #{chunk_counter}"),
                        None,
                    );
                }
            }
            Ok(Ok(DecodedChunk::EndOfStream)) => {
                reached_eos = true;
                break;
            }
            Ok(Err(err)) => {
                aria_domain::log_diagnostic(
                    aria_domain::DiagnosticSubsystem::Playback,
                    aria_domain::DiagnosticLogLevel::Error,
                    format!("Decoder worker encountered error: {err}"),
                    None,
                );
                reached_eos = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !is_paused && playback_started {
                    aria_domain::log_diagnostic(
                        aria_domain::DiagnosticSubsystem::Playback,
                        aria_domain::DiagnosticLogLevel::Warn,
                        "Audio buffer starvation: decoder lookahead queue empty (>50ms wait)",
                        None,
                    );
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                reached_eos = true;
                break;
            }
        }
    }

    // Stop and join decode thread cleanly by dropping the channel receiver first
    stop_flag.store(true, Ordering::Relaxed);
    drop(chunk_rx.take());
    let _ = decode_thread.join();

    // If we reached end of stream naturally, initiate AudioTrack drain so remaining
    // buffered audio plays out completely before signaling finished.
    if reached_eos {
        let _ = env.call_method(&player_obj, "endOfStream", "()Z", &[]);
        let _ = env.exception_clear();

        let drain_start = std::time::Instant::now();
        let max_drain_duration = Duration::from_millis(3500);

        loop {
            // Process any incoming commands during drain
            while let Ok(cmd) = commands.try_recv() {
                match cmd {
                    ExclusiveCommand::Play => {
                        let _ = env.call_method(&player_obj, "play", "()Z", &[]);
                    }
                    ExclusiveCommand::Pause => {
                        let _ = env.call_method(&player_obj, "pause", "()Z", &[]);
                    }
                    ExclusiveCommand::Terminate => {
                        let _ = env.call_method(&player_obj, "close", "()Z", &[]);
                        let _ = env.exception_clear();
                        return;
                    }
                }
            }

            // Update position during drain
            let _: Result<(), jni::errors::Error> = env.with_local_frame(16, |env| {
                if let Ok(pos_val) = env.call_method(&player_obj, "getPositionMs", "()J", &[]) {
                    if let Ok(pos_ms) = pos_val.j() {
                        if pos_ms >= 0 {
                            state.position_ms.store(pos_ms as u64, Ordering::Relaxed);
                        }
                    }
                }
                Ok(())
            });
            let _ = env.exception_clear();

            // Check if AudioTrack finished playing
            let finished = match env.with_local_frame::<_, bool, jni::errors::Error>(16, |env| {
                Ok(env.call_method(&player_obj, "isFinished", "()Z", &[])?
                    .z()?)
            }) {
                Ok(val) => val,
                Err(_) => {
                    let _ = env.exception_clear();
                    true
                }
            };

            if finished || drain_start.elapsed() > max_drain_duration {
                state.finished.store(true, Ordering::Relaxed);
                aria_domain::log_diagnostic(
                    aria_domain::DiagnosticSubsystem::Playback,
                    aria_domain::DiagnosticLogLevel::Info,
                    format!("AudioTrack drain completed in {:.2}s", drain_start.elapsed().as_secs_f64()),
                    None,
                );
                break;
            }

            thread::sleep(Duration::from_millis(15));
        }

        // Wait for Terminate command (from PlaybackService advancing or stopping)
        // or a brief timeout before closing the player object.
        loop {
            match commands.recv_timeout(Duration::from_millis(200)) {
                Ok(ExclusiveCommand::Terminate) => break,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if drain_start.elapsed() > max_drain_duration + Duration::from_secs(2) {
                        break;
                    }
                }
            }
        }
    } else {
        state.finished.store(true, Ordering::Relaxed);
    }

    let _ = env.call_method(&player_obj, "close", "()Z", &[]);
    let _ = env.exception_clear();
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

fn apply_volume(bytes: &mut [u8], active_format: ActivePcmFormat, volume: f32) {
    if volume >= 0.999_9 {
        return;
    }
    if volume <= 0.000_1 {
        bytes.fill(0);
        return;
    }

    match active_format {
        ActivePcmFormat::PcmFloat => {
            let sample_count = bytes.len() / 4;
            let samples = unsafe {
                std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut f32, sample_count)
            };
            for s in samples {
                *s *= volume;
            }
        }
        ActivePcmFormat::Pcm16 => {
            let sample_count = bytes.len() / 2;
            let samples = unsafe {
                std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i16, sample_count)
            };
            for s in samples {
                *s = (*s as f32 * volume).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            }
        }
        ActivePcmFormat::Pcm24Packed => {
            let sample_count = bytes.len() / 3;
            for i in 0..sample_count {
                let offset = i * 3;
                let b0 = bytes[offset];
                let b1 = bytes[offset + 1];
                let b2 = bytes[offset + 2];
                let sign_extend = if (b2 & 0x80) != 0 { 0xFF } else { 0x00 };
                let sample = i32::from_le_bytes([b0, b1, b2, sign_extend]);
                let scaled =
                    (sample as f64 * volume as f64).round().clamp(-8388608.0, 8388607.0) as i32;
                let out_bytes = scaled.to_le_bytes();
                bytes[offset] = out_bytes[0];
                bytes[offset + 1] = out_bytes[1];
                bytes[offset + 2] = out_bytes[2];
            }
        }
        ActivePcmFormat::Pcm32 => {
            let sample_count = bytes.len() / 4;
            let samples = unsafe {
                std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut i32, sample_count)
            };
            for s in samples {
                *s = (*s as f64 * volume as f64).round().clamp(i32::MIN as f64, i32::MAX as f64)
                    as i32;
            }
        }
    }
}

pub fn get_active_bluetooth_description() -> Option<String> {
    let (mut env, app_context, class_loader) = get_android_jni()?;

    let class_name = match env.new_string("com.aria.player.AriaBluetoothAudioHelper") {
        Ok(cn) => cn,
        Err(_) => return None,
    };

    let helper_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return None,
    };

    let helper_class: JClass = helper_class_obj.into();
    let res = match env.call_static_method(
        &helper_class,
        "getActiveBluetoothDescription",
        "(Landroid/content/Context;)Ljava/lang/String;",
        &[(&app_context).into()],
    ) {
        Ok(val) => match val.l() {
            Ok(obj) => obj,
            Err(_) => return None,
        },
        Err(_) => return None,
    };

    if res.is_null() {
        return None;
    }

    let jstr: JString = res.into();
    env.get_string(&jstr).ok().map(|s| s.into())
}

pub fn optimize_bluetooth_audio(sample_rate: u32) -> Option<String> {
    let (mut env, app_context, class_loader) = get_android_jni()?;

    let class_name = match env.new_string("com.aria.player.AriaBluetoothAudioHelper") {
        Ok(cn) => cn,
        Err(_) => return None,
    };

    let helper_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return None,
    };

    let helper_class: JClass = helper_class_obj.into();
    let res = match env.call_static_method(
        &helper_class,
        "optimizeBluetoothAudioForRate",
        "(Landroid/content/Context;I)Ljava/lang/String;",
        &[(&app_context).into(), JValue::Int(sample_rate as i32)],
    ) {
        Ok(val) => match val.l() {
            Ok(obj) => obj,
            Err(_) => return None,
        },
        Err(_) => return None,
    };

    if res.is_null() {
        return None;
    }

    let jstr: JString = res.into();
    env.get_string(&jstr).ok().map(|s| s.into())
}

pub fn optimize_bluetooth_audio_default() -> Option<String> {
    optimize_bluetooth_audio(44100)
}

pub fn has_bluetooth_output() -> bool {
    let (mut env, app_context, class_loader) = match get_android_jni() {
        Some(j) => j,
        None => return false,
    };

    let class_name = match env.new_string("com.aria.player.AriaBluetoothAudioHelper") {
        Ok(cn) => cn,
        Err(_) => return false,
    };

    let helper_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return false,
    };

    let helper_class: JClass = helper_class_obj.into();
    match env.call_static_method(
        &helper_class,
        "hasBluetoothOutput",
        "(Landroid/content/Context;)Z",
        &[(&app_context).into()],
    ) {
        Ok(v) => v.z().unwrap_or(false),
        Err(_) => false,
    }
}

pub fn is_bluetooth_device_id(device_id_int: i32) -> bool {
    let (mut env, app_context, class_loader) = match get_android_jni() {
        Some(j) => j,
        None => return false,
    };

    let class_name = match env.new_string("com.aria.player.AriaBluetoothAudioHelper") {
        Ok(cn) => cn,
        Err(_) => return false,
    };

    let helper_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return false,
    };

    let helper_class: JClass = helper_class_obj.into();
    match env.call_static_method(
        &helper_class,
        "isBluetoothDeviceId",
        "(Landroid/content/Context;I)Z",
        &[(&app_context).into(), JValue::Int(device_id_int)],
    ) {
        Ok(v) => v.z().unwrap_or(false),
        Err(_) => false,
    }
}

pub fn update_playback_service(
    title: Option<&str>,
    artist: Option<&str>,
    audio_spec: Option<&str>,
    is_playing: bool,
) {
    let (mut env, app_context, class_loader) = match get_android_jni() {
        Some(j) => j,
        None => return,
    };

    let class_name = match env.new_string("com.aria.player.AriaPlaybackService") {
        Ok(cn) => cn,
        Err(_) => return,
    };

    let service_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return,
    };

    let service_class: JClass = service_class_obj.into();
    let null_obj = JObject::null();
    let j_title = title.and_then(|t| env.new_string(t).ok());
    let j_artist = artist.and_then(|a| env.new_string(a).ok());
    let j_spec = audio_spec.and_then(|s| env.new_string(s).ok());
    let title_val: JValue = match j_title.as_ref() {
        Some(s) => s.into(),
        None => (&null_obj).into(),
    };
    let artist_val: JValue = match j_artist.as_ref() {
        Some(s) => s.into(),
        None => (&null_obj).into(),
    };
    let spec_val: JValue = match j_spec.as_ref() {
        Some(s) => s.into(),
        None => (&null_obj).into(),
    };

    let _ = env.call_static_method(
        &service_class,
        "updatePlayback",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V",
        &[
            (&app_context).into(),
            title_val,
            artist_val,
            spec_val,
            JValue::Bool(if is_playing { 1 } else { 0 }),
        ],
    );
}

pub fn stop_playback_service() {
    let (mut env, app_context, class_loader) = match get_android_jni() {
        Some(j) => j,
        None => return,
    };

    let class_name = match env.new_string("com.aria.player.AriaPlaybackService") {
        Ok(cn) => cn,
        Err(_) => return,
    };

    let service_class_obj = match env
        .call_method(
            &class_loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[(&class_name).into()],
        )
        .and_then(|v| v.l())
    {
        Ok(obj) => obj,
        Err(_) => return,
    };

    let service_class: JClass = service_class_obj.into();
    let _ = env.call_static_method(
        &service_class,
        "stopPlayback",
        "(Landroid/content/Context;)V",
        &[(&app_context).into()],
    );
}
