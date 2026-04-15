mod commands;
mod error;

use std::sync::Arc;

use aria_app_core::AppCore;
use tauri::Emitter;

pub struct AppState {
    pub core: Arc<AppCore>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let core = Arc::new(AppCore::new().expect("failed to initialize Aria core"));
    let setup_core = core.clone();

    tauri::Builder::default()
        .manage(AppState { core })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let mut events = setup_core.subscribe();

            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let _ = app_handle.emit("aria://app-event", event);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::bootstrap,
            commands::system::pick_directory,
            commands::system::open_directory,
            commands::system::show_in_explorer,
            commands::system::debug_log,
            commands::library::add_library_root,
            commands::library::remove_library_root,
            commands::library::clear_library,
            commands::library::start_library_scan,
            commands::library::set_field_mappings,
            commands::library::set_catalog_rules,
            commands::library::read_track_raw_tags,
            commands::playback::play,
            commands::playback::play_track,
            commands::playback::add_to_queue,
            commands::playback::replace_queue,
            commands::playback::previous_track,
            commands::playback::next_track,
            commands::playback::shuffle_queue,
            commands::playback::restore_queue_order,
            commands::playback::pause,
            commands::playback::seek,
            commands::playlists::create_playlist,
            commands::playlists::add_tracks_to_playlist,
            commands::playlists::rename_playlist,
            commands::playlists::delete_playlist,
            commands::playlists::regenerate_playlist_icon,
            commands::playlists::export_playlist_m3u,
            commands::playlists::remove_tracks_from_playlist,
            commands::settings::update_theme,
            commands::settings::update_track_table_settings,
            commands::settings::update_album_track_table_settings,
            commands::settings::update_playlist_track_table_settings,
            commands::settings::list_output_devices,
            commands::settings::update_playback_preferences
        ])
        .run(tauri::generate_context!())
        .expect("error while running aria");
}
