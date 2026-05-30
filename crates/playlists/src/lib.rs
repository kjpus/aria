use std::sync::Arc;

use aria_domain::{Playlist, PlaylistSnapshot};
use chrono::Utc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Error)]
pub enum PlaylistError {
    #[error("playlist name cannot be empty")]
    EmptyName,
    #[error("a playlist named '{0}' already exists")]
    DuplicateName(String),
    #[error("playlist not found")]
    PlaylistNotFound,
    #[error("cannot delete or rename the automatic Favorites playlist")]
    ModifyFavoritesRestricted,
}

#[derive(Clone, Default)]
pub struct PlaylistService {
    state: Arc<RwLock<PlaylistSnapshot>>,
}

impl PlaylistService {
    pub fn with_snapshot(mut snapshot: PlaylistSnapshot) -> Self {
        if !snapshot.playlists.iter().any(|p| p.id == "favorites") {
            if let Some(existing) = snapshot
                .playlists
                .iter_mut()
                .find(|p| p.name.eq_ignore_ascii_case("Favorites"))
            {
                existing.id = "favorites".to_string();
            } else {
                snapshot.playlists.push(Playlist {
                    id: "favorites".to_string(),
                    name: "Favorites".to_string(),
                    collage_seed: 0,
                    track_ids: Vec::new(),
                    created_at: Some(Utc::now().to_rfc3339()),
                });
            }
        }
        Self {
            state: Arc::new(RwLock::new(snapshot)),
        }
    }

    pub async fn snapshot(&self) -> PlaylistSnapshot {
        self.state.read().await.clone()
    }

    pub async fn create_playlist(
        &self,
        name: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        let name = sanitize_playlist_name(name)?;
        let mut state = self.state.write().await;

        if state
            .playlists
            .iter()
            .any(|playlist| playlist_name_eq(&playlist.name, &name))
        {
            return Err(PlaylistError::DuplicateName(name));
        }

        state.playlists.push(Playlist {
            id: build_playlist_id(&name),
            name,
            collage_seed: 0,
            track_ids: dedupe_preserve_order(track_ids),
            created_at: Some(Utc::now().to_rfc3339()),
        });

        Ok(state.clone())
    }

    pub async fn add_tracks(
        &self,
        playlist_id: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        let mut state = self.state.write().await;
        let playlist = state
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == playlist_id)
            .ok_or(PlaylistError::PlaylistNotFound)?;

        for track_id in track_ids {
            if !playlist.track_ids.iter().any(|existing| existing == &track_id) {
                playlist.track_ids.push(track_id);
            }
        }

        Ok(state.clone())
    }

    pub async fn rename_playlist(
        &self,
        playlist_id: String,
        name: String,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        if playlist_id == "favorites" {
            return Err(PlaylistError::ModifyFavoritesRestricted);
        }
        let name = sanitize_playlist_name(name)?;
        let mut state = self.state.write().await;

        if state.playlists.iter().any(|playlist| {
            playlist.id != playlist_id && playlist_name_eq(&playlist.name, &name)
        }) {
            return Err(PlaylistError::DuplicateName(name));
        }

        let playlist = state
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == playlist_id)
            .ok_or(PlaylistError::PlaylistNotFound)?;
        playlist.name = name;
        Ok(state.clone())
    }

    pub async fn delete_playlist(
        &self,
        playlist_id: String,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        if playlist_id == "favorites" {
            return Err(PlaylistError::ModifyFavoritesRestricted);
        }
        let mut state = self.state.write().await;
        let original_len = state.playlists.len();
        state.playlists.retain(|playlist| playlist.id != playlist_id);
        if state.playlists.len() == original_len {
            return Err(PlaylistError::PlaylistNotFound);
        }

        Ok(state.clone())
    }

    pub async fn regenerate_icon(
        &self,
        playlist_id: String,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        let mut state = self.state.write().await;
        let playlist = state
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == playlist_id)
            .ok_or(PlaylistError::PlaylistNotFound)?;
        playlist.collage_seed = playlist.collage_seed.wrapping_add(1);
        Ok(state.clone())
    }

    pub async fn remove_tracks(
        &self,
        playlist_id: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, PlaylistError> {
        let mut state = self.state.write().await;
        let playlist = state
            .playlists
            .iter_mut()
            .find(|playlist| playlist.id == playlist_id)
            .ok_or(PlaylistError::PlaylistNotFound)?;
        playlist
            .track_ids
            .retain(|existing| !track_ids.iter().any(|track_id| track_id == existing));
        Ok(state.clone())
    }

    pub async fn clear(&self) -> PlaylistSnapshot {
        let mut state = self.state.write().await;
        state.playlists.clear();
        state.playlists.push(Playlist {
            id: "favorites".to_string(),
            name: "Favorites".to_string(),
            collage_seed: 0,
            track_ids: Vec::new(),
            created_at: Some(Utc::now().to_rfc3339()),
        });
        state.clone()
    }
}

fn sanitize_playlist_name(name: String) -> Result<String, PlaylistError> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err(PlaylistError::EmptyName);
    }

    Ok(trimmed)
}

fn build_playlist_id(name: &str) -> String {
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let suffix = Utc::now().timestamp_millis();
    if slug.is_empty() {
        format!("playlist-{suffix}")
    } else {
        format!("{slug}-{suffix}")
    }
}

fn playlist_name_eq(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();

    for value in values {
        if !deduped.iter().any(|existing| existing == &value) {
            deduped.push(value);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_domain::PlaylistSnapshot;

    #[tokio::test]
    async fn test_favorites_playlist_creation_and_restrictions() {
        // Initialize empty snapshot
        let snapshot = PlaylistSnapshot::default();
        let service = PlaylistService::with_snapshot(snapshot);

        // Verify Favorites playlist was automatically created
        let current_state = service.snapshot().await;
        assert_eq!(current_state.playlists.len(), 1);
        assert_eq!(current_state.playlists[0].id, "favorites");
        assert_eq!(current_state.playlists[0].name, "Favorites");

        // Verify renaming Favorites is rejected
        let rename_res = service.rename_playlist("favorites".to_string(), "New Name".to_string()).await;
        assert!(matches!(rename_res, Err(PlaylistError::ModifyFavoritesRestricted)));

        // Verify deleting Favorites is rejected
        let delete_res = service.delete_playlist("favorites".to_string()).await;
        assert!(matches!(delete_res, Err(PlaylistError::ModifyFavoritesRestricted)));

        // Verify clearing playlists preserves Favorites
        let cleared_state = service.clear().await;
        assert_eq!(cleared_state.playlists.len(), 1);
        assert_eq!(cleared_state.playlists[0].id, "favorites");
    }

    #[tokio::test]
    async fn test_favorites_playlist_migration() {
        // Initialize snapshot with an existing "Favorites" playlist with a different ID
        let mut snapshot = PlaylistSnapshot::default();
        snapshot.playlists.push(Playlist {
            id: "favorites-12345".to_string(),
            name: "Favorites".to_string(),
            collage_seed: 0,
            track_ids: vec!["track-1".to_string()],
            created_at: None,
        });

        let service = PlaylistService::with_snapshot(snapshot);

        // Verify it was migrated to the fixed ID "favorites" and didn't duplicate
        let current_state = service.snapshot().await;
        assert_eq!(current_state.playlists.len(), 1);
        assert_eq!(current_state.playlists[0].id, "favorites");
        assert_eq!(current_state.playlists[0].name, "Favorites");
        assert_eq!(current_state.playlists[0].track_ids, vec!["track-1"]);
    }
}
