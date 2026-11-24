use aria_domain::RemoteTarget;

pub(crate) fn track_belongs_to_album_folder(
    track_remote_path: &str,
    album_remote_path: &str,
) -> bool {
    let normalized_album = album_remote_path.replace('\\', "/");
    let album_trimmed = normalized_album.trim_matches('/');
    if album_trimmed.is_empty() {
        return false;
    }
    let normalized_track = track_remote_path.replace('\\', "/");
    let track_trimmed = normalized_track.trim_start_matches('/');
    track_trimmed
        .strip_prefix(album_trimmed)
        .is_some_and(|rest| rest.starts_with('/'))
}

const REMOTE_CONFIG_SECRET_KEYS: &[&str] = &[
    "password",
    "clientSecret",
    "client_secret",
    "accessToken",
    "access_token",
    "refreshToken",
    "refresh_token",
];

fn has_non_empty_string(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> bool {
    map.get(key)
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty())
}

pub(crate) fn redact_remote_target_for_ui(mut target: RemoteTarget) -> RemoteTarget {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&target.config_json) else {
        return target;
    };
    if let Some(map) = value.as_object_mut() {
        let had_password = has_non_empty_string(map, "password");
        let had_tokens = has_non_empty_string(map, "accessToken")
            || has_non_empty_string(map, "access_token")
            || has_non_empty_string(map, "refreshToken")
            || has_non_empty_string(map, "refresh_token");

        for key in REMOTE_CONFIG_SECRET_KEYS {
            map.remove(*key);
        }
        if had_password {
            map.insert("hasPassword".to_string(), serde_json::Value::Bool(true));
        }
        if had_tokens {
            map.insert("hasTokens".to_string(), serde_json::Value::Bool(true));
        }

        if let Ok(serialized) = serde_json::to_string(&value) {
            target.config_json = serialized;
        }
    }
    target
}

pub(crate) fn merge_preserved_remote_secrets(
    incoming: &mut RemoteTarget,
    existing: Option<&RemoteTarget>,
) {
    let Ok(mut incoming_val) = serde_json::from_str::<serde_json::Value>(&incoming.config_json) else {
        return;
    };
    let Some(incoming_map) = incoming_val.as_object_mut() else {
        return;
    };

    incoming_map.remove("hasPassword");
    incoming_map.remove("hasTokens");

    if let Some(existing_target) = existing {
        if let Ok(serde_json::Value::Object(existing_map)) =
            serde_json::from_str::<serde_json::Value>(&existing_target.config_json)
        {
            for key in REMOTE_CONFIG_SECRET_KEYS {
                let incoming_missing_or_empty = !has_non_empty_string(incoming_map, key);
                if incoming_missing_or_empty {
                    if let Some(secret_val) = existing_map.get(*key) {
                        if secret_val.as_str().is_some_and(|s| !s.is_empty()) {
                            incoming_map.insert((*key).to_string(), secret_val.clone());
                        }
                    }
                }
            }
        }
    }

    if let Ok(serialized) = serde_json::to_string(&incoming_val) {
        incoming.config_json = serialized;
    }
}
