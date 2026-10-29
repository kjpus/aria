use std::{
    fs::File,
    io::ErrorKind,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use jni::{
    objects::{JClass, JObject, JString, JValue},
    JavaVM,
};
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

#[derive(Clone, Copy, PartialEq, Eq)]
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
}

impl ExclusivePlayer {
    pub fn open(
        device_id: &str,
        path: &str,
        start_position: Duration,
        start_paused: bool,
        volume: f32,
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
            .spawn(move || {
                run_android_exclusive_thread(
                    device_id_int,
                    decoder,
                    stream_spec,
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
            Ok(Err(error)) => Err(error),
            Err(_) => Err(PlaybackError::Output(format!(
                "Failed to initialize Android bit-accurate playback for '{path}'"
            ))),
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

        Ok(Self {
            format,
            decoder,
            track_id: track.id,
            stream_spec,
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
    stop_flag: Arc<AtomicBool>,
) -> (
    JoinHandle<()>,
    Receiver<Result<DecodedChunk, PlaybackError>>,
) {
    let (tx, rx) =
        mpsc::sync_channel::<Result<DecodedChunk, PlaybackError>>(LOOKAHEAD_QUEUE_CAPACITY);

    let handle = thread::Builder::new()
        .name("aria-android-decode".into())
        .spawn(move || {
            let mut pending_buffer = Vec::new();

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                let packet = match decoder.format.next_packet() {
                    Ok(packet) => packet,
                    Err(SymphoniaError::IoError(error))
                        if error.kind() == ErrorKind::UnexpectedEof =>
                    {
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
                        write_audio_buffer_bytes(
                            &mut pending_buffer,
                            audio_buffer,
                            active_format,
                        );

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
    match active_format {
        ActivePcmFormat::PcmFloat => {
            let mut buffer =
                SampleBuffer::<f32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            destination.extend_from_slice(as_bytes(buffer.samples()));
        }
        ActivePcmFormat::Pcm16 => {
            let mut buffer =
                SampleBuffer::<i16>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            destination.extend_from_slice(as_bytes(buffer.samples()));
        }
        ActivePcmFormat::Pcm24Packed => {
            let mut buffer =
                SampleBuffer::<i32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            for &sample in buffer.samples() {
                let bytes = sample.to_le_bytes();
                destination.extend_from_slice(&bytes[0..3]);
            }
        }
        ActivePcmFormat::Pcm32 => {
            let mut buffer =
                SampleBuffer::<i32>::new(audio_buffer.frames() as u64, *audio_buffer.spec());
            buffer.copy_interleaved_ref(audio_buffer);
            destination.extend_from_slice(as_bytes(buffer.samples()));
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
    state: Arc<ExclusiveState>,
    commands: Receiver<ExclusiveCommand>,
    init_tx: Sender<Result<(), PlaybackError>>,
) {
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(vm) => vm,
        Err(err) => {
            let _ = init_tx.send(Err(PlaybackError::Output(format!("Failed to get JavaVM: {err}"))));
            return;
        }
    };

    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(err) => {
            let _ = init_tx.send(Err(PlaybackError::Output(format!("Failed to attach JNI thread: {err}"))));
            return;
        }
    };

    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    // In Android JNI, threads attached via attach_current_thread() use the system ClassLoader,
    // which cannot find application APK classes with env.find_class().
    // We must load application classes using app_context.getClassLoader().loadClass(...).
    let class_loader = match env.call_method(
        &app_context,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        &[],
    ) {
        Ok(val) => match val.l() {
            Ok(obj) => obj,
            Err(err) => {
                let _ = env.exception_clear();
                let _ = init_tx.send(Err(PlaybackError::Output(format!(
                    "Failed to get ClassLoader object: {err}"
                ))));
                return;
            }
        },
        Err(err) => {
            let _ = env.exception_clear();
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to call getClassLoader(): {err}"
            ))));
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
    let format_result = env.call_method(
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
    );

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
        Ok(fmt) => fmt,
        Err(_) => {
            let _ = init_tx.send(Err(PlaybackError::Output(format!(
                "Failed to open Android bit-accurate stream (sampleRate={}Hz, bits={}, ch={})",
                stream_spec.sample_rate, bit_depth, stream_spec.channels
            ))));
            return;
        }
    };

    if !start_paused {
        let _ = env.call_method(&player_obj, "play", "()Z", &[]);
    }

    let _ = init_tx.send(Ok(()));

    // Spawn the background decode worker
    let stop_flag = Arc::new(AtomicBool::new(false));
    let (decode_thread, chunk_rx) = spawn_decode_worker(decoder, active_format, stop_flag.clone());
    let mut chunk_rx = Some(chunk_rx);

    let mut is_paused = start_paused;
    let mut reached_eos = false;

    while !reached_eos {
        // Handle pending commands non-blocking
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                ExclusiveCommand::Play => {
                    is_paused = false;
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

                // Transfer data to JNI byte array and write
                if let Ok(j_bytes) = env.byte_array_from_slice(&data) {
                    let _ = env.call_method(
                        &player_obj,
                        "write",
                        "([BII)I",
                        &[
                            (&j_bytes).into(),
                            JValue::Int(0),
                            JValue::Int(data.len() as i32),
                        ],
                    );
                    let _ = env.exception_clear();
                }

                // Update position
                if let Ok(pos_val) = env.call_method(&player_obj, "getPositionMs", "()J", &[]) {
                    if let Ok(pos_ms) = pos_val.j() {
                        if pos_ms >= 0 {
                            state.position_ms.store(pos_ms as u64, Ordering::Relaxed);
                        }
                    }
                } else {
                    let _ = env.exception_clear();
                }
            }
            Ok(Ok(DecodedChunk::EndOfStream)) => {
                reached_eos = true;
                break;
            }
            Ok(Err(_)) => {
                reached_eos = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
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
        let max_drain_duration = Duration::from_secs(15);

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
            if let Ok(pos_val) = env.call_method(&player_obj, "getPositionMs", "()J", &[]) {
                if let Ok(pos_ms) = pos_val.j() {
                    if pos_ms >= 0 {
                        state.position_ms.store(pos_ms as u64, Ordering::Relaxed);
                    }
                }
            } else {
                let _ = env.exception_clear();
            }

            // Check if AudioTrack finished playing
            let finished = match env.call_method(&player_obj, "isFinished", "()Z", &[]) {
                Ok(val) => val.z().unwrap_or(true),
                Err(_) => {
                    let _ = env.exception_clear();
                    true
                }
            };

            if finished || drain_start.elapsed() > max_drain_duration {
                state.finished.store(true, Ordering::Relaxed);
                break;
            }

            thread::sleep(Duration::from_millis(30));
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
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return None,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return None,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return None,
    };

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
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return None,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return None,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return None,
    };

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
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return false,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return false,
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
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return false,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return false,
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

pub fn update_playback_service(title: Option<&str>, artist: Option<&str>, is_playing: bool) {
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return,
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
    let title_val: JValue = match j_title.as_ref() {
        Some(s) => s.into(),
        None => (&null_obj).into(),
    };
    let artist_val: JValue = match j_artist.as_ref() {
        Some(s) => s.into(),
        None => (&null_obj).into(),
    };

    let _ = env.call_static_method(
        &service_class,
        "updatePlayback",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Z)V",
        &[
            (&app_context).into(),
            title_val,
            artist_val,
            JValue::Bool(if is_playing { 1 } else { 0 }),
        ],
    );
}

pub fn stop_playback_service() {
    let ctx = ndk_context::android_context();
    let vm = match unsafe { JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(v) => v,
        Err(_) => return,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(e) => e,
        Err(_) => return,
    };
    let app_context = unsafe { JObject::from_raw(ctx.context().cast()) };

    let class_loader = match env
        .call_method(&app_context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|v| v.l())
    {
        Ok(cl) => cl,
        Err(_) => return,
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
