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
}

#[derive(Clone, Default)]
pub struct PlaylistService {
    state: Arc<RwLock<PlaylistSnapshot>>,
}

impl PlaylistService {
    pub fn with_snapshot(snapshot: PlaylistSnapshot) -> Self {
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
