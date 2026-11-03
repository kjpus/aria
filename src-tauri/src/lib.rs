mod commands;
mod error;

use std::sync::Arc;

use aria_app_core::AppCore;
use tauri::Emitter;

pub struct AppState {
    pub core: Arc<AppCore>,
}

static GLOBAL_APP_CORE: std::sync::OnceLock<Arc<AppCore>> = std::sync::OnceLock::new();
static MAIN_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

pub fn get_app_core() -> Option<Arc<AppCore>> {
    GLOBAL_APP_CORE.get().cloned()
}

pub fn get_main_runtime() -> &'static tokio::runtime::Runtime {
    MAIN_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .thread_stack_size(4 * 1024 * 1024)
            .enable_all()
            .build()
            .expect("failed to initialize main tokio runtime")
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = get_main_runtime();
    let _guard = rt.enter();

    let core = Arc::new(AppCore::new().expect("failed to initialize Aria core"));
    let _ = GLOBAL_APP_CORE.set(core.clone());
    let setup_core = core.clone();
    let run_core = core.clone();

    tauri::Builder::default()
        .manage(AppState { core })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let mut events = setup_core.subscribe();

            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(event) => {
                            let _ = app_handle.emit("aria://app-event", event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                            eprintln!("[tauri] app-event forwarder lagged by {missed} events; continuing");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::bootstrap,
            commands::system::pick_directory,
            commands::system::open_directory,
            commands::system::open_url,
            commands::system::show_in_explorer,
            commands::system::debug_log,
            commands::library::add_library_root,
            commands::library::remove_library_root,
            commands::library::clear_library,
            commands::library::start_library_scan,
            commands::library::set_field_mappings,
            commands::library::set_catalog_rules,
            commands::library::read_track_raw_tags,
            commands::library::export_field_to_tag,
            commands::library::edit_track_tags,
            commands::playback::play,
            commands::playback::play_track,
            commands::playback::add_to_queue,
            commands::playback::replace_queue,
            commands::playback::previous_track,
            commands::playback::next_track,
            commands::playback::play_queue_index,
            commands::playback::shuffle_queue,
            commands::playback::restore_queue_order,
            commands::playback::pause,
            commands::playback::stop,
            commands::playback::seek,
            commands::playlists::create_playlist,
            commands::playlists::add_tracks_to_playlist,
            commands::playlists::rename_playlist,
            commands::playlists::delete_playlist,
            commands::playlists::regenerate_playlist_icon,
            commands::playlists::export_playlist_m3u,
            commands::playlists::pick_playlist_file,
            commands::playlists::get_playlist_import_preview,
            commands::playlists::commit_playlist_import,
            commands::playlists::remove_tracks_from_playlist,
            commands::playlists::reorder_playlist_tracks,
            commands::playlists::reorder_playlists,
            commands::settings::update_theme,
            commands::settings::update_track_table_settings,
            commands::settings::update_album_track_table_settings,
            commands::settings::update_playlist_track_table_settings,
            commands::settings::list_output_devices,
            commands::settings::refresh_output_devices,
            commands::settings::update_playback_preferences,
            commands::settings::update_auto_sync_on_startup,
            commands::settings::update_remote_storage_only,
            commands::settings::update_logging_settings,
            commands::settings::get_diagnostic_logs,
            commands::settings::clear_diagnostic_logs,
            commands::settings::export_diagnostic_logs_text,
            commands::settings::get_diagnostic_log_path,
            commands::remote::get_remote_targets,
            commands::remote::save_remote_target,
            commands::remote::delete_remote_target,
            commands::remote::clear_remote_target,
            commands::remote::test_remote_target,
            commands::remote::start_gdrive_auth_flow,
            commands::remote::complete_gdrive_auth_flow,
            commands::remote::upload_album_to_target,
            commands::remote::upload_playlist_to_target,
            commands::remote::delete_album_from_target,
            commands::remote::delete_playlist_from_target,
            commands::remote::get_remote_synced_items,
            commands::remote::get_remote_cache_status,
            commands::remote::clear_remote_cache,
            commands::remote::sync_remote_target,
            commands::remote::sync_all_remote_targets
        ])
        .build(tauri::generate_context!())
        .expect("error while building aria")
        .run(move |_app_handle, event| match event {
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { .. },
                ..
            }
            | tauri::RunEvent::ExitRequested { .. }
            | tauri::RunEvent::Exit => {
                run_core.shutdown();
            }
            _ => {}
        });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativePlayPause(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    if let Some(core) = get_app_core() {
        let rt = get_main_runtime();
        rt.spawn(async move {
            let snapshot = core.playback_snapshot().await;
            if snapshot.status == aria_domain::PlaybackStatus::Playing {
                let _ = core.pause().await;
            } else {
                let _ = core.play().await;
            }
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeNext(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    if let Some(core) = get_app_core() {
        let rt = get_main_runtime();
        rt.spawn(async move {
            let _ = core.next_track().await;
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativePrevious(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    if let Some(core) = get_app_core() {
        let rt = get_main_runtime();
        rt.spawn(async move {
            let _ = core.previous_track().await;
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeStopPlayback(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    if let Some(core) = get_app_core() {
        let rt = get_main_runtime();
        rt.spawn(async move {
            let _ = core.pause().await;
        });
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeRegisterContext(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    context: jni::objects::JObject,
) {
    aria_app_core::register_android_context(&mut env, &context);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeLogDiagnostic(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    level_int: jni::sys::jint,
    subsystem_str: jni::objects::JString,
    message_str: jni::objects::JString,
) {
    let subsystem: String = match env.get_string(&subsystem_str) {
        Ok(s) => s.into(),
        Err(_) => "General".into(),
    };
    let message: String = match env.get_string(&message_str) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let level = match level_int {
        0 => aria_domain::DiagnosticLogLevel::Trace,
        1 => aria_domain::DiagnosticLogLevel::Debug,
        2 => aria_domain::DiagnosticLogLevel::Info,
        3 => aria_domain::DiagnosticLogLevel::Warn,
        4 => aria_domain::DiagnosticLogLevel::Error,
        _ => aria_domain::DiagnosticLogLevel::Info,
    };
    let sub = aria_domain::DiagnosticSubsystem::from_str_loose(&subsystem);
    aria_domain::log_diagnostic(sub, level, message, None);
}
