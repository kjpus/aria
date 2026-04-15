#[cfg(target_os = "windows")]
#[path = "exclusive_wasapi.rs"]
mod exclusive_wasapi;

use std::{
    fs::File,
    io::BufReader,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use aria_domain::{
    OutputDeviceSnapshot, PlayTrackRequest, PlaybackPreferences, PlaybackSessionSnapshot,
    PlaybackSnapshot, PlaybackStatus,
};
use rand::{seq::SliceRandom, thread_rng};
use rodio::{
    cpal::{self, traits::DeviceTrait, traits::HostTrait, DeviceDescription, DeviceId},
    Decoder, DeviceSinkBuilder, MixerDeviceSink, Player,
};
use thiserror::Error;
use tokio::sync::RwLock;

#[cfg(target_os = "windows")]
use self::exclusive_wasapi::{is_wasapi_device_id, ExclusivePlayer};

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("Unable to open the default output device")]
    NoOutputDevice,
    #[error("The selected output device is no longer available: {id}")]
    UnknownOutputDevice { id: String },
    #[error("Exclusive mode is not available on this platform or for this output device")]
    ExclusiveModeUnavailable,
    #[error("Unable to open audio output: {0}")]
    Output(String),
    #[error("Unable to open track for playback: {path}")]
    OpenTrack {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Unable to decode track for playback: {path}")]
    DecodeTrack { path: String },
}

#[derive(Clone)]
pub struct PlaybackService {
    state: Arc<RwLock<PlaybackSnapshot>>,
    backend: Arc<Mutex<PlaybackBackend>>,
}

#[derive(Debug, Clone)]
pub struct OutputDeviceRefresh {
    pub playback_snapshot: PlaybackSnapshot,
    pub updated_preferences: Option<PlaybackPreferences>,
}

struct PlaybackBackend {
    shared_output: Option<MixerDeviceSink>,
    output_device: OutputDeviceSnapshot,
    active: Option<ActivePlayback>,
    last_request: Option<PlayTrackRequest>,
    queue: Vec<PlayTrackRequest>,
    ordered_queue: Vec<PlayTrackRequest>,
    current_queue_index: Option<usize>,
    preferences: PlaybackPreferences,
}

enum ActivePlayback {
    Shared {
        player: Player,
        request: PlayTrackRequest,
    },
    #[cfg(target_os = "windows")]
    Exclusive {
        player: ExclusivePlayer,
        request: PlayTrackRequest,
    },
}

struct OutputTarget {
    device: cpal::Device,
    snapshot: OutputDeviceSnapshot,
}

struct ResumePlayback {
    request: PlayTrackRequest,
    position: Duration,
    paused: bool,
}

impl ActivePlayback {
    fn play(&self) {
        match self {
            Self::Shared { player, .. } => player.play(),
            #[cfg(target_os = "windows")]
            Self::Exclusive { player, .. } => player.play(),
        }
    }

    fn pause(&self) {
        match self {
            Self::Shared { player, .. } => player.pause(),
            #[cfg(target_os = "windows")]
            Self::Exclusive { player, .. } => player.pause(),
        }
    }

    fn is_paused(&self) -> bool {
        match self {
            Self::Shared { player, .. } => player.is_paused(),
            #[cfg(target_os = "windows")]
            Self::Exclusive { player, .. } => player.is_paused(),
        }
    }

    fn get_pos(&self) -> Duration {
        match self {
            Self::Shared { player, .. } => player.get_pos(),
            #[cfg(target_os = "windows")]
            Self::Exclusive { player, .. } => player.get_pos(),
        }
    }

    fn empty(&self) -> bool {
        match self {
            Self::Shared { player, .. } => player.empty(),
            #[cfg(target_os = "windows")]
            Self::Exclusive { player, .. } => player.empty(),
        }
    }

    fn request(&self) -> &PlayTrackRequest {
        match self {
            Self::Shared { request, .. } => request,
            #[cfg(target_os = "windows")]
            Self::Exclusive { request, .. } => request,
        }
    }
}

impl Default for PlaybackBackend {
    fn default() -> Self {
        let preferences = PlaybackPreferences::default();
        let output_device = resolve_output_snapshot(&preferences, false)
            .unwrap_or_else(|_| OutputDeviceSnapshot::default());

        Self {
            shared_output: None,
            output_device,
            active: None,
            last_request: None,
            queue: Vec::new(),
            ordered_queue: Vec::new(),
            current_queue_index: None,
            preferences,
        }
    }
}

impl Default for PlaybackService {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackService {
    pub fn new() -> Self {
        Self::with_session(
            PlaybackPreferences::default(),
            PlaybackSessionSnapshot::default(),
        )
    }

    pub fn with_preferences(preferences: PlaybackPreferences) -> Self {
        Self::with_session(preferences, PlaybackSessionSnapshot::default())
    }

    pub fn with_session(
        preferences: PlaybackPreferences,
        session: PlaybackSessionSnapshot,
    ) -> Self {
        let output_device = resolve_output_snapshot(&preferences, false)
            .unwrap_or_else(|_| OutputDeviceSnapshot::default());
        let normalized_session = normalize_session(session);
        let current_track = normalized_session
            .current_queue_index
            .and_then(|index| normalized_session.queue.get(index))
            .map(|request| request.queue_item.clone());
        let snapshot = PlaybackSnapshot {
            status: PlaybackStatus::Stopped,
            current_track,
            queue: normalized_session
                .queue
                .iter()
                .map(|request| request.queue_item.clone())
                .collect(),
            current_queue_index: normalized_session.current_queue_index,
            queue_depth: normalized_session.queue.len(),
            position_ms: 0,
            output_device: output_device.clone(),
        };

        Self {
            state: Arc::new(RwLock::new(snapshot)),
            backend: Arc::new(Mutex::new(PlaybackBackend {
                shared_output: None,
                output_device,
                active: None,
                last_request: normalized_session
                    .current_queue_index
                    .and_then(|index| normalized_session.queue.get(index))
                    .cloned()
                    .or_else(|| normalized_session.queue.first().cloned()),
                queue: normalized_session.queue,
                ordered_queue: normalized_session.ordered_queue,
                current_queue_index: normalized_session.current_queue_index,
                preferences,
            })),
        }
    }

    pub async fn snapshot(&self) -> PlaybackSnapshot {
        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        state.clone()
    }

    pub fn persisted_session(&self) -> PlaybackSessionSnapshot {
        let backend = self.backend.lock().expect("playback backend poisoned");
        PlaybackSessionSnapshot {
            queue: backend.queue.clone(),
            ordered_queue: backend.ordered_queue.clone(),
            current_queue_index: backend.current_queue_index,
        }
    }

    pub fn list_output_devices(&self) -> Result<Vec<OutputDeviceSnapshot>, PlaybackError> {
        list_output_devices()
    }

    pub async fn handle_output_device_change(
        &self,
        devices: &[OutputDeviceSnapshot],
    ) -> Result<Option<OutputDeviceRefresh>, PlaybackError> {
        let mut updated_preferences = None;
        let mut changed = false;

        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            let previous_preferences = backend.preferences.clone();
            let active_device_available = device_exists(devices, &backend.output_device.id);
            let selected_device_missing = backend
                .preferences
                .output_device_id
                .as_ref()
                .is_some_and(|device_id| !device_exists(devices, device_id));

            if selected_device_missing || (backend.active.is_some() && !active_device_available) {
                let mut fallback_preferences = backend.preferences.clone();
                fallback_preferences.output_device_id = None;
                if fallback_preferences != previous_preferences {
                    updated_preferences = Some(fallback_preferences.clone());
                }
                rebind_output(&mut backend, fallback_preferences.clone(), false)?;
                changed = true;
            } else if let Some(current_snapshot) = devices
                .iter()
                .find(|device| device.id == backend.output_device.id)
                .cloned()
            {
                let merged_snapshot =
                    snapshot_with_backend_label(current_snapshot, backend_backend_label(&backend));
                if merged_snapshot != backend.output_device {
                    backend.output_device = merged_snapshot;
                    changed = true;
                }
            } else if let Some(default_snapshot) =
                devices.iter().find(|device| device.is_default).cloned()
            {
                let merged_snapshot =
                    snapshot_with_backend_label(default_snapshot, backend_backend_label(&backend));
                if merged_snapshot != backend.output_device {
                    backend.output_device = merged_snapshot;
                    changed = true;
                }
            }
        }

        if !changed {
            return Ok(None);
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(Some(OutputDeviceRefresh {
            playback_snapshot: state.clone(),
            updated_preferences,
        }))
    }

    pub async fn update_preferences(
        &self,
        preferences: PlaybackPreferences,
    ) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            rebind_output(&mut backend, preferences, true)?;
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn play(&self) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            if let Some(active) = backend.active.as_ref() {
                active.play();
            } else if let Some(request) = current_queue_request(&backend)
                .cloned()
                .or_else(|| backend.last_request.clone())
            {
                start_request(&mut backend, request, Duration::ZERO, false)?;
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn play_track(
        &self,
        request: PlayTrackRequest,
    ) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            backend.queue = vec![request.clone()];
            backend.ordered_queue = backend.queue.clone();
            backend.current_queue_index = Some(0);
            start_request(&mut backend, request, Duration::ZERO, false)?;
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn add_to_queue(&self, requests: Vec<PlayTrackRequest>) -> PlaybackSnapshot {
        if !requests.is_empty() {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            let queue_was_empty = backend.queue.is_empty();
            backend.queue.extend(requests.clone());
            backend.ordered_queue.extend(requests);
            if backend.current_queue_index.is_none() {
                backend.current_queue_index = Some(0);
            }
            if queue_was_empty {
                backend.last_request = backend.queue.first().cloned();
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        state.clone()
    }

    pub async fn replace_queue(
        &self,
        requests: Vec<PlayTrackRequest>,
        start_playing: bool,
    ) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            backend.active = None;
            backend.queue = requests;
            backend.ordered_queue = backend.queue.clone();
            backend.current_queue_index = (!backend.queue.is_empty()).then_some(0);
            backend.last_request = backend.queue.first().cloned();

            if start_playing {
                if let Some(request) = current_queue_request(&backend).cloned() {
                    start_request(&mut backend, request, Duration::ZERO, false)?;
                }
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn previous_track(&self) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            let restart_current = backend
                .active
                .as_ref()
                .is_some_and(|active| active.get_pos() >= Duration::from_secs(3));

            if restart_current {
                if let Some(request) = current_queue_request(&backend).cloned() {
                    start_request(&mut backend, request, Duration::ZERO, false)?;
                }
            } else if let Some(current_index) = backend.current_queue_index {
                let target_index = current_index.saturating_sub(1);
                jump_to_queue_index(&mut backend, target_index, false)?;
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn next_track(&self) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            if let Some(current_index) = backend.current_queue_index {
                if current_index + 1 < backend.queue.len() {
                    jump_to_queue_index(&mut backend, current_index + 1, false)?;
                }
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub async fn shuffle_queue(&self) -> PlaybackSnapshot {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            let start_index = backend
                .current_queue_index
                .map_or(0, |index| index.saturating_add(1));
            if start_index < backend.queue.len() {
                backend.queue[start_index..].shuffle(&mut thread_rng());
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        state.clone()
    }

    pub async fn restore_queue_order(&self) -> PlaybackSnapshot {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            let start_index = backend
                .current_queue_index
                .map_or(0, |index| index.saturating_add(1));
            if start_index <= backend.queue.len()
                && backend.queue.len() == backend.ordered_queue.len()
            {
                let restored_queue_tail = backend.ordered_queue[start_index..].to_vec();
                backend.queue[start_index..].clone_from_slice(&restored_queue_tail);
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        state.clone()
    }

    pub async fn pause(&self) -> PlaybackSnapshot {
        {
            let backend = self.backend.lock().expect("playback backend poisoned");
            if let Some(active) = backend.active.as_ref() {
                active.pause();
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        state.clone()
    }

    pub async fn seek(&self, position_ms: u64) -> Result<PlaybackSnapshot, PlaybackError> {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            if let Some((request, paused)) = seek_target(&backend) {
                let clamped_position = position_ms.min(request.queue_item.duration_ms);
                start_request(
                    &mut backend,
                    request,
                    Duration::from_millis(clamped_position),
                    paused,
                )?;
            }
        }

        let mut state = self.state.write().await;
        self.refresh_state(&mut state);
        Ok(state.clone())
    }

    pub fn shutdown(&self) {
        {
            let mut backend = self.backend.lock().expect("playback backend poisoned");
            backend.active = None;
            backend.shared_output = None;
        }

        let mut state = self.state.blocking_write();
        self.refresh_state(&mut state);
        state.status = PlaybackStatus::Stopped;
        state.position_ms = 0;
    }

    fn refresh_state(&self, state: &mut PlaybackSnapshot) {
        let mut backend = self.backend.lock().expect("playback backend poisoned");
        sync_snapshot_queue_state(state, &backend);

        if let Some((position_ms, request, status, finished)) =
            backend.active.as_ref().map(|active| {
                let position_ms = active.get_pos().as_millis() as u64;
                (
                    position_ms,
                    active.request().clone(),
                    if active.is_paused() {
                        PlaybackStatus::Paused
                    } else {
                        PlaybackStatus::Playing
                    },
                    playback_finished(active, position_ms),
                )
            })
        {
            state.status = status;
            state.current_track = Some(request.queue_item.clone());
            state.position_ms = position_ms.min(request.queue_item.duration_ms);

            if finished {
                backend.last_request = Some(request.clone());

                if let Some(next_request) = advance_queue_after_finish(&mut backend) {
                    match start_request(&mut backend, next_request, Duration::ZERO, false) {
                        Ok(()) => {
                            sync_snapshot_queue_state(state, &backend);
                            if let Some(active) = backend.active.as_ref() {
                                state.status = if active.is_paused() {
                                    PlaybackStatus::Paused
                                } else {
                                    PlaybackStatus::Playing
                                };
                                state.current_track = Some(active.request().queue_item.clone());
                                state.position_ms = (active.get_pos().as_millis() as u64)
                                    .min(active.request().queue_item.duration_ms);
                                return;
                            }
                        }
                        Err(error) => {
                            eprintln!("failed to continue queued playback: {error}");
                        }
                    }
                }

                backend.active = None;
                sync_snapshot_queue_state(state, &backend);
                state.status = PlaybackStatus::Stopped;
                state.position_ms = request.queue_item.duration_ms;
            }
            return;
        }

        if let Some(request) = current_queue_request(&backend) {
            state.status = PlaybackStatus::Stopped;
            if state
                .current_track
                .as_ref()
                .is_none_or(|item| item.id != request.queue_item.id)
            {
                state.current_track = Some(request.queue_item.clone());
                state.position_ms = 0;
            }
            return;
        }

        if let Some(request) = backend.last_request.as_ref() {
            state.status = PlaybackStatus::Stopped;
            if state
                .current_track
                .as_ref()
                .is_none_or(|item| item.id != request.queue_item.id)
            {
                state.current_track = Some(request.queue_item.clone());
                state.position_ms = 0;
            }
            return;
        }

        state.status = PlaybackStatus::Stopped;
        state.current_track = None;
        state.position_ms = 0;
    }
}

fn start_request(
    backend: &mut PlaybackBackend,
    request: PlayTrackRequest,
    start_position: Duration,
    start_paused: bool,
) -> Result<(), PlaybackError> {
    backend.active = None;

    if backend.preferences.exclusive_mode {
        #[cfg(target_os = "windows")]
        {
            match resolve_output_target(&backend.preferences, false).and_then(|target| {
                ExclusivePlayer::open(
                    &target.snapshot.id,
                    &request.path,
                    start_position,
                    start_paused,
                )
                .map(|player| (player, target.snapshot))
            }) {
                Ok((player, snapshot)) => {
                    backend.shared_output = None;
                    backend.last_request = Some(request.clone());
                    backend.output_device = snapshot;
                    backend.active = Some(ActivePlayback::Exclusive { player, request });
                    return Ok(());
                }
                Err(error) => {
                    eprintln!(
                        "exclusive playback failed for '{}' on '{}': {error}",
                        request.path,
                        backend
                            .preferences
                            .output_device_id
                            .as_deref()
                            .unwrap_or("default output")
                    );
                    return Err(error);
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            eprintln!(
                "exclusive playback requested for '{}', but this platform does not support it",
                request.path
            );
            return Err(PlaybackError::ExclusiveModeUnavailable);
        }
    }

    if backend.shared_output.is_none() {
        let (output, output_device) = open_shared_output(&backend.preferences, false)?;
        backend.shared_output = Some(output);
        backend.output_device = output_device;
    }

    let file = File::open(&request.path).map_err(|source| PlaybackError::OpenTrack {
        path: request.path.clone(),
        source,
    })?;
    let decoder =
        Decoder::try_from(BufReader::new(file)).map_err(|_| PlaybackError::DecodeTrack {
            path: request.path.clone(),
        })?;

    let player = Player::connect_new(
        &backend
            .shared_output
            .as_ref()
            .expect("shared output created")
            .mixer(),
    );
    player.append(decoder);
    if start_position > Duration::ZERO {
        let _ = player.try_seek(start_position);
    }
    if start_paused {
        player.pause();
    }

    backend.last_request = Some(request.clone());
    backend.active = Some(ActivePlayback::Shared { player, request });
    Ok(())
}

fn jump_to_queue_index(
    backend: &mut PlaybackBackend,
    index: usize,
    start_paused: bool,
) -> Result<(), PlaybackError> {
    let Some(request) = backend.queue.get(index).cloned() else {
        return Ok(());
    };

    backend.current_queue_index = Some(index);
    start_request(backend, request, Duration::ZERO, start_paused)
}

fn seek_target(backend: &PlaybackBackend) -> Option<(PlayTrackRequest, bool)> {
    if let Some(active) = backend.active.as_ref() {
        return Some((active.request().clone(), active.is_paused()));
    }

    if let Some(request) = current_queue_request(backend) {
        return Some((request.clone(), true));
    }

    backend.last_request.clone().map(|request| (request, true))
}

fn current_queue_request(backend: &PlaybackBackend) -> Option<&PlayTrackRequest> {
    backend
        .current_queue_index
        .and_then(|index| backend.queue.get(index))
}

fn normalize_session(mut session: PlaybackSessionSnapshot) -> PlaybackSessionSnapshot {
    if session.ordered_queue.len() != session.queue.len() {
        session.ordered_queue = session.queue.clone();
    }

    session.current_queue_index = session
        .current_queue_index
        .filter(|index| *index < session.queue.len())
        .or_else(|| (!session.queue.is_empty()).then_some(0));

    session
}

fn advance_queue_after_finish(backend: &mut PlaybackBackend) -> Option<PlayTrackRequest> {
    let next_index = backend
        .current_queue_index
        .and_then(|index| (index + 1 < backend.queue.len()).then_some(index + 1))?;
    backend.current_queue_index = Some(next_index);
    backend.queue.get(next_index).cloned()
}

fn playback_finished(active: &ActivePlayback, position_ms: u64) -> bool {
    if active.empty() {
        return true;
    }

    let duration_ms = active.request().queue_item.duration_ms;
    duration_ms > 0 && position_ms >= duration_ms
}

fn sync_snapshot_queue_state(state: &mut PlaybackSnapshot, backend: &PlaybackBackend) {
    state.output_device = backend.output_device.clone();
    state.queue = backend
        .queue
        .iter()
        .map(|request| request.queue_item.clone())
        .collect();
    state.current_queue_index = backend.current_queue_index;
    state.queue_depth = state.queue.len();
}

fn rebind_output(
    backend: &mut PlaybackBackend,
    preferences: PlaybackPreferences,
    strict_selected: bool,
) -> Result<(), PlaybackError> {
    if preferences.exclusive_mode && !exclusive_capable_for_preferences(&preferences) {
        return Err(PlaybackError::ExclusiveModeUnavailable);
    }

    let resume = backend.active.as_ref().map(|active| ResumePlayback {
        request: active.request().clone(),
        position: active.get_pos(),
        paused: active.is_paused(),
    });

    let resolved_snapshot = resolve_output_snapshot(&preferences, strict_selected)?;
    backend.preferences = preferences;
    backend.shared_output = None;
    backend.output_device = resolved_snapshot;
    backend.active = None;

    if let Some(resume) = resume {
        start_request(backend, resume.request, resume.position, resume.paused)?;
    }

    Ok(())
}

fn open_shared_output(
    preferences: &PlaybackPreferences,
    strict_selected: bool,
) -> Result<(MixerDeviceSink, OutputDeviceSnapshot), PlaybackError> {
    let target = resolve_output_target(&shared_mode_preferences(preferences), strict_selected)?;
    let mut output = DeviceSinkBuilder::from_device(target.device)
        .map_err(|error| PlaybackError::Output(error.to_string()))?
        .open_sink_or_fallback()
        .map_err(|error| PlaybackError::Output(error.to_string()))?;
    output.log_on_drop(false);
    Ok((output, target.snapshot))
}

fn list_output_devices() -> Result<Vec<OutputDeviceSnapshot>, PlaybackError> {
    let host = cpal::default_host();
    let default_device_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    let mut devices = host
        .output_devices()
        .map_err(|error| PlaybackError::Output(error.to_string()))?
        .filter_map(|device| {
            output_device_snapshot(&device, default_device_id.as_deref(), false).ok()
        })
        .collect::<Vec<_>>();

    devices.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(devices)
}

fn resolve_output_snapshot(
    preferences: &PlaybackPreferences,
    strict_selected: bool,
) -> Result<OutputDeviceSnapshot, PlaybackError> {
    resolve_output_target(preferences, strict_selected).map(|target| target.snapshot)
}

fn resolve_output_target(
    preferences: &PlaybackPreferences,
    strict_selected: bool,
) -> Result<OutputTarget, PlaybackError> {
    if preferences.exclusive_mode && !exclusive_capable_for_preferences(preferences) {
        return Err(PlaybackError::ExclusiveModeUnavailable);
    }

    let host = cpal::default_host();
    let default_device_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());

    if let Some(requested_id) = preferences.output_device_id.as_ref() {
        if let Some(device) = output_device_by_id(&host, requested_id) {
            let snapshot = output_device_snapshot(
                &device,
                default_device_id.as_deref(),
                preferences.exclusive_mode,
            )?;
            return Ok(OutputTarget { device, snapshot });
        }

        if strict_selected {
            return Err(PlaybackError::UnknownOutputDevice {
                id: requested_id.clone(),
            });
        }
    }

    let device = host
        .default_output_device()
        .ok_or(PlaybackError::NoOutputDevice)?;
    let snapshot = output_device_snapshot(
        &device,
        default_device_id.as_deref(),
        preferences.exclusive_mode,
    )?;
    Ok(OutputTarget { device, snapshot })
}

fn output_device_by_id(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    DeviceId::from_str(id)
        .ok()
        .and_then(|device_id| host.device_by_id(&device_id))
}

fn output_device_snapshot(
    device: &cpal::Device,
    default_device_id: Option<&str>,
    exclusive_mode: bool,
) -> Result<OutputDeviceSnapshot, PlaybackError> {
    let description = device
        .description()
        .map_err(|error| PlaybackError::Output(error.to_string()))?;
    let id = device
        .id()
        .map_err(|error| PlaybackError::Output(error.to_string()))?
        .to_string();

    Ok(OutputDeviceSnapshot {
        is_default: default_device_id.is_some_and(|default_id| default_id == id),
        id: id.clone(),
        name: preferred_output_device_name(&description),
        backend: output_backend_label(exclusive_mode),
        exclusive_capable: is_exclusive_capable_device_id(&id),
    })
}

fn preferred_output_device_name(description: &DeviceDescription) -> String {
    let primary = description.name().trim();

    for candidate in description.extended() {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() && !same_label(trimmed, primary) {
            return trimmed.to_string();
        }
    }

    if let Some(driver) = description.driver() {
        let trimmed = driver.trim();
        if !trimmed.is_empty() && !same_label(trimmed, primary) {
            return format!("{primary} ({trimmed})");
        }
    }

    primary.to_string()
}

fn same_label(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn exclusive_capable_for_preferences(preferences: &PlaybackPreferences) -> bool {
    if !preferences.exclusive_mode {
        return true;
    }

    match preferences.output_device_id.as_deref() {
        Some(device_id) => is_exclusive_capable_device_id(device_id),
        None => cfg!(target_os = "windows"),
    }
}

fn is_exclusive_capable_device_id(device_id: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        is_wasapi_device_id(device_id)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = device_id;
        false
    }
}

fn output_backend_label(exclusive_mode: bool) -> String {
    if cfg!(target_os = "windows") {
        if exclusive_mode {
            "WASAPI exclusive".into()
        } else {
            "rodio / WASAPI shared".into()
        }
    } else {
        "rodio".into()
    }
}

fn backend_backend_label(backend: &PlaybackBackend) -> String {
    match backend.active {
        #[cfg(target_os = "windows")]
        Some(ActivePlayback::Exclusive { .. }) => output_backend_label(true),
        Some(ActivePlayback::Shared { .. }) => output_backend_label(false),
        None => output_backend_label(backend.preferences.exclusive_mode),
    }
}

fn snapshot_with_backend_label(
    mut snapshot: OutputDeviceSnapshot,
    backend_label: String,
) -> OutputDeviceSnapshot {
    snapshot.backend = backend_label;
    snapshot
}

fn shared_mode_preferences(preferences: &PlaybackPreferences) -> PlaybackPreferences {
    let mut shared = preferences.clone();
    shared.exclusive_mode = false;
    shared
}

fn device_exists(devices: &[OutputDeviceSnapshot], device_id: &str) -> bool {
    devices.iter().any(|device| device.id == device_id)
}
