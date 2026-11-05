#[cfg(target_os = "android")]
use crate::{get_main_runtime, get_or_init_app_core};
#[cfg(target_os = "android")]
use jni::{
    objects::{JClass, JObject, JString},
    sys::{jint, jlong, jstring},
    JNIEnv,
};

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoEnsureAppCore(
    mut env: JNIEnv,
    _class: JClass,
    context: JObject,
) {
    aria_app_core::register_android_context(&mut env, &context);
    let _ = get_or_init_app_core();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetQueue(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_queue().await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetAlbums(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_albums().await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetAlbumTracks(
    mut env: JNIEnv,
    _class: JClass,
    album_id: JString,
) -> jstring {
    let id_str: String = match env.get_string(&album_id) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_album_tracks(&id_str).await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetPlaylists(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_playlists().await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetPlaylistTracks(
    mut env: JNIEnv,
    _class: JClass,
    playlist_id: JString,
) -> jstring {
    let id_str: String = match env.get_string(&playlist_id) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_playlist_tracks(&id_str).await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetComposers(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_composers().await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetComposerAlbums(
    mut env: JNIEnv,
    _class: JClass,
    composer: JString,
) -> jstring {
    let comp_str: String = match env.get_string(&composer) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let items = rt.block_on(async move { core.auto_get_composer_albums(&comp_str).await });
    let json = serde_json::to_string(&items).unwrap_or_else(|_| "[]".into());
    match env.new_string(json) {
        Ok(js) => js.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlay(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.play().await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPause(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.pause().await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayPause(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
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

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoNext(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.next_track().await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPrevious(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.previous_track().await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoStop(
    _env: JNIEnv,
    _class: JClass,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.stop().await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoSeekTo(
    _env: JNIEnv,
    _class: JClass,
    position_ms: jlong,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.seek(position_ms.max(0) as u64).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayQueueIndex(
    _env: JNIEnv,
    _class: JClass,
    index: jint,
) {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.auto_play_queue_index(index.max(0) as usize).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayAlbum(
    mut env: JNIEnv,
    _class: JClass,
    album_id: JString,
) {
    let id_str: String = match env.get_string(&album_id) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.auto_play_album(&id_str).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayPlaylist(
    mut env: JNIEnv,
    _class: JClass,
    playlist_id: JString,
) {
    let id_str: String = match env.get_string(&playlist_id) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.auto_play_playlist(&id_str).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayTrack(
    mut env: JNIEnv,
    _class: JClass,
    track_id: JString,
    album_id: JString,
) {
    let t_id: String = match env.get_string(&track_id) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let alb_id_opt: Option<String> = if album_id.is_null() {
        None
    } else {
        env.get_string(&album_id).ok().map(|s| s.into())
    };

    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.auto_play_track(&t_id, alb_id_opt.as_deref()).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoPlayFromSearch(
    mut env: JNIEnv,
    _class: JClass,
    query: JString,
) {
    let q_str: String = match env.get_string(&query) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    rt.spawn(async move {
        let _ = core.auto_play_from_search(&q_str).await;
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn Java_com_aria_player_AriaPlaybackService_nativeAutoGetNowPlaying(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let core = get_or_init_app_core();
    let rt = get_main_runtime();
    let info = rt.block_on(async move { core.auto_get_now_playing().await });
    match info {
        Some(info) => {
            let json = serde_json::to_string(&info).unwrap_or_else(|_| "{}".into());
            match env.new_string(json) {
                Ok(js) => js.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        None => std::ptr::null_mut(),
    }
}
