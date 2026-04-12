use std::{
    ffi::OsStr,
    fs::File,
    io::BufReader,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use rodio::{Decoder, Source};
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
}

enum ExclusiveCommand {
    Play,
    Pause,
    Terminate,
}

struct ExclusiveInit {
    decoder: Decoder<BufReader<File>>,
    audio_client: Audio::IAudioClient,
    render_client: Audio::IAudioRenderClient,
    render_event: Foundation::HANDLE,
    buffer_frames: u32,
    channels: u16,
    sample_rate: u32,
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
    ) -> Result<Self, PlaybackError> {
        let endpoint_id = wasapi_endpoint_id(device_id)
            .ok_or_else(|| PlaybackError::Output(format!("unsupported Windows device id: {device_id}")))?;
        let state = Arc::new(ExclusiveState {
            paused: AtomicBool::new(start_paused),
            finished: AtomicBool::new(false),
            position_ms: AtomicU64::new(start_position.as_millis() as u64),
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
}

impl Drop for ExclusivePlayer {
    fn drop(&mut self) {
        let _ = self.commands.send(ExclusiveCommand::Terminate);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
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
    let com_initialized = unsafe {
        Com::CoInitializeEx(None, Com::COINIT_MULTITHREADED).is_ok()
    };

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
    let mut frames_rendered = position_to_frames(start_position, init.sample_rate);

    if !source_finished {
        match fill_render_buffer(
            &init.render_client,
            init.buffer_frames,
            init.channels,
            init.sample_rate,
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

        let wait_result = unsafe {
            Threading::WaitForSingleObject(init.render_event, COMMAND_POLL_MS)
        };

        match wait_result {
            WAIT_OBJECT_0 if playing && !source_finished => {
                match current_padding(&init.audio_client) {
                    Ok(padding) => {
                        let frames_available = init.buffer_frames.saturating_sub(padding);
                        if frames_available == 0 {
                            continue;
                        }

                        match fill_render_buffer(
                            &init.render_client,
                            frames_available,
                            init.channels,
                            init.sample_rate,
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
                    Err(error) => {
                        eprintln!("exclusive playback padding read failed: {error}");
                        state.finished.store(true, Ordering::Relaxed);
                        cleanup_and_uninitialize(com_initialized, init.audio_client, init.render_event);
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
    let decoder = open_decoder(path, start_position)?;
    let channels = decoder.channels().get();
    let sample_rate = decoder.sample_rate().get();

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

    let audio_client: Audio::IAudioClient = unsafe {
        device
            .Activate(Com::CLSCTX_ALL, None)
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    let wave_format = build_wave_format(channels, sample_rate);
    let format_result = unsafe {
        audio_client.IsFormatSupported(
            Audio::AUDCLNT_SHAREMODE_EXCLUSIVE,
            &wave_format.Format,
            None,
        )
    };

    if format_result.is_err() {
        return Err(PlaybackError::Output(format!(
            "WASAPI exclusive does not support {sample_rate} Hz / {channels} ch float output for this device"
        )));
    }

    let mut default_period = 0_i64;
    let mut minimum_period = 0_i64;
    unsafe {
        audio_client
            .GetDevicePeriod(Some(&mut default_period), Some(&mut minimum_period))
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
    }
    let requested_period = if default_period > 0 {
        default_period
    } else {
        minimum_period
    };

    let render_event = unsafe {
        Threading::CreateEventA(None, false, false, windows::core::PCSTR(ptr::null()))
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };

    unsafe {
        audio_client
            .Initialize(
                Audio::AUDCLNT_SHAREMODE_EXCLUSIVE,
                Audio::AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                requested_period,
                requested_period,
                &wave_format.Format,
                None,
            )
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
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

    Ok(ExclusiveInit {
        decoder,
        audio_client,
        render_client,
        render_event,
        buffer_frames,
        channels,
        sample_rate,
    })
}

fn open_decoder(
    path: &str,
    start_position: Duration,
) -> Result<Decoder<BufReader<File>>, PlaybackError> {
    let file = File::open(path).map_err(|source| PlaybackError::OpenTrack {
        path: path.to_string(),
        source,
    })?;
    let byte_len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let reader = BufReader::new(file);
    let mut builder = Decoder::builder()
        .with_data(reader)
        .with_seekable(true);

    if byte_len > 0 {
        builder = builder.with_byte_len(byte_len);
    }

    if let Some(hint) = Path::new(path).extension().and_then(|extension| extension.to_str()) {
        if !hint.is_empty() {
            builder = builder.with_hint(hint);
        }
    }

    let mut decoder = builder.build().map_err(|_| PlaybackError::DecodeTrack {
        path: path.to_string(),
    })?;

    if start_position > Duration::ZERO {
        let _ = decoder.try_seek(start_position);
    }

    Ok(decoder)
}

fn fill_render_buffer(
    render_client: &Audio::IAudioRenderClient,
    frames_available: u32,
    channels: u16,
    sample_rate: u32,
    decoder: &mut Decoder<BufReader<File>>,
    state: &ExclusiveState,
    frames_rendered: &mut u64,
) -> Result<bool, PlaybackError> {
    let sample_count = frames_available as usize * channels as usize;
    let buffer = unsafe {
        render_client
            .GetBuffer(frames_available)
            .map_err(|error| PlaybackError::Output(error.to_string()))?
    };
    let samples = unsafe {
        std::slice::from_raw_parts_mut(buffer as *mut f32, sample_count)
    };

    let mut samples_written = 0usize;
    while samples_written < sample_count {
        match decoder.next() {
            Some(sample) => {
                samples[samples_written] = sample;
                samples_written += 1;
            }
            None => break,
        }
    }

    if samples_written < sample_count {
        samples[samples_written..].fill(0.0);
    }

    unsafe {
        render_client
            .ReleaseBuffer(frames_available, 0)
            .map_err(|error| PlaybackError::Output(error.to_string()))?;
    }

    let frames_written_now = samples_written / channels as usize;
    *frames_rendered += frames_written_now as u64;
    state.position_ms.store(
        frames_to_millis(*frames_rendered, sample_rate),
        Ordering::Relaxed,
    );

    Ok(samples_written < sample_count)
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
    ((position.as_secs_f64() * sample_rate as f64).round() as u64).max(0)
}

fn frames_to_millis(frames: u64, sample_rate: u32) -> u64 {
    ((frames as f64 / sample_rate as f64) * 1000.0).round() as u64
}

fn build_wave_format(channels: u16, sample_rate: u32) -> Audio::WAVEFORMATEXTENSIBLE {
    let sample_bytes = std::mem::size_of::<f32>() as u16;
    let block_align = channels * sample_bytes;
    let bits_per_sample = 8 * sample_bytes;
    let cb_size =
        (std::mem::size_of::<Audio::WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<Audio::WAVEFORMATEX>())
            as u16;

    Audio::WAVEFORMATEXTENSIBLE {
        Format: Audio::WAVEFORMATEX {
            wFormatTag: KernelStreaming::WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: channels,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: channels as u32 * sample_rate * sample_bytes as u32,
            nBlockAlign: block_align,
            wBitsPerSample: bits_per_sample,
            cbSize: cb_size,
        },
        Samples: Audio::WAVEFORMATEXTENSIBLE_0 {
            wSamplesPerBlock: bits_per_sample,
        },
        dwChannelMask: KernelStreaming::KSAUDIO_SPEAKER_DIRECTOUT,
        SubFormat: Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
    }
}
