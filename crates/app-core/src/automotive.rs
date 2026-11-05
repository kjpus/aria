use std::collections::{BTreeMap, HashMap};
use serde::{Deserialize, Serialize};

use aria_domain::{PlaybackSnapshot, PlayTrackRequest, QueueItem, ScannedTrack};
use aria_library::manifest::album_id_for_track;

use crate::{AppCore, AppCoreError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub duration_ms: u64,
    pub is_playable: bool,
    pub is_browsable: bool,
    pub artwork_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutoNowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub spec: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub is_playing: bool,
    pub track_id: Option<String>,
    pub album_id: Option<String>,
    pub artwork_path: Option<String>,
}

pub fn first_field<'a>(track: &'a ScannedTrack, key: &str) -> Option<&'a str> {
    track
        .mapped_fields
        .get(key)
        .and_then(|v| v.first())
        .map(|s| s.as_str())
}

pub fn join_field(track: &ScannedTrack, key: &str) -> String {
    track
        .mapped_fields
        .get(key)
        .map(|v| v.join(" / "))
        .unwrap_or_default()
}

pub fn track_display_title(track: &ScannedTrack) -> String {
    let movement = first_field(track, "movement");
    let work = first_field(track, "work");
    let title = first_field(track, "title");

    if let (Some(m), Some(w)) = (movement, work) {
        if !m.is_empty() && !w.is_empty() && m != w {
            return format!("{w}: {m}");
        }
    }
    if let Some(m) = movement {
        if !m.is_empty() {
            return m.to_string();
        }
    }
    if let Some(t) = title {
        if !t.is_empty() {
            return t.to_string();
        }
    }
    track.file_name.clone()
}

pub fn track_display_subtitle(track: &ScannedTrack) -> String {
    let composer = join_field(track, "composer");
    let conductor = join_field(track, "conductor");
    let ensemble = join_field(track, "ensemble");
    let soloist = join_field(track, "soloist");

    let mut credits = Vec::new();
    if !conductor.is_empty() {
        credits.push(conductor);
    }
    if !ensemble.is_empty() {
        credits.push(ensemble);
    }
    if !soloist.is_empty() {
        credits.push(soloist);
    }
    let perf = credits.join(" / ");

    if !composer.is_empty() && !perf.is_empty() {
        format!("{composer} • {perf}")
    } else if !composer.is_empty() {
        composer
    } else if !perf.is_empty() {
        perf
    } else {
        join_field(track, "artist")
    }
}

pub fn parse_number(s: &str) -> u32 {
    s.split('/').next().unwrap_or("0").trim().parse::<u32>().unwrap_or(0)
}

pub fn track_sort_key(track: &ScannedTrack) -> (u32, u32, String) {
    let disc = first_field(track, "disc_number")
        .or_else(|| first_field(track, "disk_number"))
        .map(parse_number)
        .unwrap_or(1);
    let track_num = first_field(track, "track_number")
        .map(parse_number)
        .unwrap_or(0);
    (disc, track_num, track.file_name.clone())
}

pub fn create_play_request(track: &ScannedTrack) -> PlayTrackRequest {
    PlayTrackRequest {
        path: track.path.clone(),
        queue_item: QueueItem {
            id: track.id.clone(),
            title: track_display_title(track),
            subtitle: track_display_subtitle(track),
            duration_ms: track.audio.duration_ms,
        },
    }
}

pub struct AlbumGroup {
    pub id: String,
    pub title: String,
    pub composer: String,
    pub credit: String,
    pub artwork_path: Option<String>,
    pub total_duration_ms: u64,
    pub tracks: Vec<ScannedTrack>,
}

pub fn group_library_albums(tracks: &[ScannedTrack]) -> Vec<AlbumGroup> {
    let mut album_map: HashMap<String, usize> = HashMap::new();
    let mut groups: Vec<AlbumGroup> = Vec::new();

    for track in tracks {
        let alb_id = album_id_for_track(track);
        let title = first_field(track, "album")
            .filter(|s| !s.is_empty())
            .unwrap_or(&track.file_name)
            .to_string();
        let composer = join_field(track, "composer");
        let conductor = join_field(track, "conductor");
        let ensemble = join_field(track, "ensemble");
        let soloist = join_field(track, "soloist");
        let mut perf_parts = Vec::new();
        if !conductor.is_empty() {
            perf_parts.push(conductor);
        }
        if !ensemble.is_empty() {
            perf_parts.push(ensemble);
        }
        if !soloist.is_empty() {
            perf_parts.push(soloist);
        }
        let credit = perf_parts.join(" / ");

        if let Some(&idx) = album_map.get(&alb_id) {
            let grp = &mut groups[idx];
            grp.total_duration_ms += track.audio.duration_ms;
            if grp.artwork_path.is_none() && track.album_art_path.is_some() {
                grp.artwork_path = track.album_art_path.clone();
            }
            if grp.composer.is_empty() && !composer.is_empty() {
                grp.composer = composer;
            }
            if grp.credit.is_empty() && !credit.is_empty() {
                grp.credit = credit;
            }
            grp.tracks.push(track.clone());
        } else {
            let idx = groups.len();
            album_map.insert(alb_id.clone(), idx);
            groups.push(AlbumGroup {
                id: alb_id,
                title,
                composer,
                credit,
                artwork_path: track.album_art_path.clone(),
                total_duration_ms: track.audio.duration_ms,
                tracks: vec![track.clone()],
            });
        }
    }

    // Sort tracks within each album by disc/track number
    for grp in &mut groups {
        grp.tracks.sort_by_key(track_sort_key);
    }

    // Sort albums alphabetically by title
    groups.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    groups
}

impl AppCore {
    /// Get the current playback queue formatted for automotive display
    pub async fn auto_get_queue(&self) -> Vec<AutoItem> {
        let snapshot = self.playback.snapshot().await;
        let library = self.library.snapshot().await;
        let art_map: HashMap<&str, &str> = library
            .tracks
            .iter()
            .filter_map(|t| t.album_art_path.as_deref().map(|p| (t.id.as_str(), p)))
            .collect();

        snapshot
            .queue
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let artwork_path = art_map.get(item.id.as_str()).map(|s| s.to_string());
                AutoItem {
                    id: format!("queue:{index}"),
                    title: item.title,
                    subtitle: item.subtitle,
                    duration_ms: item.duration_ms,
                    is_playable: true,
                    is_browsable: false,
                    artwork_path,
                }
            })
            .collect()
    }

    /// Get all albums for automotive browsing
    pub async fn auto_get_albums(&self) -> Vec<AutoItem> {
        let library = self.library.snapshot().await;
        let groups = group_library_albums(&library.tracks);

        groups
            .into_iter()
            .map(|grp| {
                let mut sub = grp.composer.clone();
                if !grp.credit.is_empty() {
                    if !sub.is_empty() {
                        sub.push_str(" • ");
                    }
                    sub.push_str(&grp.credit);
                }
                if sub.is_empty() {
                    sub = format!("{} tracks", grp.tracks.len());
                }

                AutoItem {
                    id: format!("album:{}", grp.id),
                    title: grp.title,
                    subtitle: sub,
                    duration_ms: grp.total_duration_ms,
                    is_playable: false,
                    is_browsable: true,
                    artwork_path: grp.artwork_path,
                }
            })
            .collect()
    }

    /// Get all tracks for a specific album
    pub async fn auto_get_album_tracks(&self, album_id: &str) -> Vec<AutoItem> {
        let library = self.library.snapshot().await;
        let mut album_tracks: Vec<ScannedTrack> = library
            .tracks
            .into_iter()
            .filter(|t| album_id_for_track(t) == album_id)
            .collect();

        album_tracks.sort_by_key(track_sort_key);

        album_tracks
            .into_iter()
            .map(|track| AutoItem {
                id: format!("track:{}:album:{}", track.id, album_id),
                title: track_display_title(&track),
                subtitle: track_display_subtitle(&track),
                duration_ms: track.audio.duration_ms,
                is_playable: true,
                is_browsable: false,
                artwork_path: track.album_art_path,
            })
            .collect()
    }

    /// Get all user playlists for automotive browsing
    pub async fn auto_get_playlists(&self) -> Vec<AutoItem> {
        let playlists_snapshot = self.playlists.snapshot().await;
        let library = self.library.snapshot().await;
        let track_art: HashMap<&str, &str> = library
            .tracks
            .iter()
            .filter_map(|t| t.album_art_path.as_deref().map(|p| (t.id.as_str(), p)))
            .collect();

        playlists_snapshot
            .playlists
            .into_iter()
            .map(|pl| {
                let first_art = pl
                    .track_ids
                    .iter()
                    .find_map(|tid| track_art.get(tid.as_str()).map(|s| s.to_string()));
                AutoItem {
                    id: format!("playlist:{}", pl.id),
                    title: pl.name,
                    subtitle: format!("{} tracks", pl.track_ids.len()),
                    duration_ms: 0,
                    is_playable: false,
                    is_browsable: true,
                    artwork_path: first_art,
                }
            })
            .collect()
    }

    /// Get all tracks for a specific playlist
    pub async fn auto_get_playlist_tracks(&self, playlist_id: &str) -> Vec<AutoItem> {
        let playlists_snapshot = self.playlists.snapshot().await;
        let playlist = match playlists_snapshot.playlists.into_iter().find(|p| p.id == playlist_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        let library = self.library.snapshot().await;
        let track_map: HashMap<&str, &ScannedTrack> = library
            .tracks
            .iter()
            .map(|t| (t.id.as_str(), t))
            .collect();

        playlist
            .track_ids
            .into_iter()
            .filter_map(|tid| track_map.get(tid.as_str()).cloned())
            .map(|track| AutoItem {
                id: format!("track:{}:playlist:{}", track.id, playlist_id),
                title: track_display_title(track),
                subtitle: track_display_subtitle(track),
                duration_ms: track.audio.duration_ms,
                is_playable: true,
                is_browsable: false,
                artwork_path: track.album_art_path.clone(),
            })
            .collect()
    }

    /// Get all composers in the library for automotive browsing
    pub async fn auto_get_composers(&self) -> Vec<AutoItem> {
        let library = self.library.snapshot().await;
        let mut composer_map: BTreeMap<String, (usize, Option<String>)> = BTreeMap::new();

        for track in &library.tracks {
            let composer = join_field(track, "composer");
            if !composer.is_empty() {
                let entry = composer_map.entry(composer).or_insert((0, None));
                entry.0 += 1;
                if entry.1.is_none() && track.album_art_path.is_some() {
                    entry.1 = track.album_art_path.clone();
                }
            }
        }

        composer_map
            .into_iter()
            .map(|(composer, (count, art))| AutoItem {
                id: format!("composer:{}", composer),
                title: composer,
                subtitle: format!("{} tracks", count),
                duration_ms: 0,
                is_playable: false,
                is_browsable: true,
                artwork_path: art,
            })
            .collect()
    }

    /// Get albums featuring a specific composer
    pub async fn auto_get_composer_albums(&self, composer: &str) -> Vec<AutoItem> {
        let library = self.library.snapshot().await;
        let groups = group_library_albums(&library.tracks);
        let comp_lower = composer.to_lowercase();

        groups
            .into_iter()
            .filter(|grp| grp.composer.to_lowercase().contains(&comp_lower))
            .map(|grp| AutoItem {
                id: format!("album:{}", grp.id),
                title: grp.title,
                subtitle: if !grp.credit.is_empty() {
                    format!("{} • {}", grp.composer, grp.credit)
                } else {
                    grp.composer
                },
                duration_ms: grp.total_duration_ms,
                is_playable: false,
                is_browsable: true,
                artwork_path: grp.artwork_path,
            })
            .collect()
    }

    /// Play a specific index in the current queue
    pub async fn auto_play_queue_index(&self, index: usize) -> Result<PlaybackSnapshot, AppCoreError> {
        self.play_queue_index(index).await
    }

    /// Play an entire album starting from track 1
    pub async fn auto_play_album(&self, album_id: &str) -> Result<PlaybackSnapshot, AppCoreError> {
        let library = self.library.snapshot().await;
        let mut tracks: Vec<ScannedTrack> = library
            .tracks
            .into_iter()
            .filter(|t| album_id_for_track(t) == album_id)
            .collect();

        if tracks.is_empty() {
            return Err(AppCoreError::Playback(aria_playback::PlaybackError::Output(
                format!("Album not found: {album_id}"),
            )));
        }

        tracks.sort_by_key(track_sort_key);
        let requests: Vec<PlayTrackRequest> = tracks.iter().map(create_play_request).collect();

        self.replace_queue(requests, true).await
    }

    /// Play an entire playlist starting from track 1
    pub async fn auto_play_playlist(&self, playlist_id: &str) -> Result<PlaybackSnapshot, AppCoreError> {
        let playlists = self.playlists.snapshot().await;
        let playlist = playlists
            .playlists
            .into_iter()
            .find(|p| p.id == playlist_id)
            .ok_or_else(|| {
                AppCoreError::Playback(aria_playback::PlaybackError::Output(format!(
                    "Playlist not found: {playlist_id}"
                )))
            })?;

        let library = self.library.snapshot().await;
        let track_map: HashMap<&str, &ScannedTrack> = library
            .tracks
            .iter()
            .map(|t| (t.id.as_str(), t))
            .collect();

        let requests: Vec<PlayTrackRequest> = playlist
            .track_ids
            .into_iter()
            .filter_map(|tid| track_map.get(tid.as_str()).map(|t| create_play_request(t)))
            .collect();

        if requests.is_empty() {
            return Err(AppCoreError::Playback(aria_playback::PlaybackError::Output(
                format!("Playlist is empty: {playlist_id}"),
            )));
        }

        self.replace_queue(requests, true).await
    }

    /// Play a specific track, optionally providing the album context so that the entire album is queued
    pub async fn auto_play_track(
        &self,
        track_id: &str,
        album_id: Option<&str>,
    ) -> Result<PlaybackSnapshot, AppCoreError> {
        let library = self.library.snapshot().await;

        if let Some(alb_id) = album_id {
            let mut tracks: Vec<ScannedTrack> = library
                .tracks
                .into_iter()
                .filter(|t| album_id_for_track(t) == alb_id)
                .collect();
            tracks.sort_by_key(track_sort_key);

            let initial_idx = tracks
                .iter()
                .position(|t| t.id == track_id)
                .unwrap_or(0);

            let requests: Vec<PlayTrackRequest> = tracks.iter().map(create_play_request).collect();
            if initial_idx == 0 {
                self.replace_queue(requests, true).await
            } else {
                self.replace_queue(requests, false).await?;
                self.play_queue_index(initial_idx).await
            }
        } else {
            let track = library
                .tracks
                .into_iter()
                .find(|t| t.id == track_id)
                .ok_or_else(|| {
                    AppCoreError::Playback(aria_playback::PlaybackError::Output(format!(
                        "Track not found: {track_id}"
                    )))
                })?;
            let request = create_play_request(&track);
            self.play_track(request).await
        }
    }

    /// Search the library and play the best match (voice search action)
    pub async fn auto_play_from_search(&self, query: &str) -> Result<PlaybackSnapshot, AppCoreError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.play().await;
        }

        let q_lower = trimmed.to_lowercase();
        let library = self.library.snapshot().await;
        let groups = group_library_albums(&library.tracks);

        // 1. Try matching composer name
        for grp in &groups {
            if grp.composer.to_lowercase().contains(&q_lower) {
                return self.auto_play_album(&grp.id).await;
            }
        }

        // 2. Try matching album title
        for grp in &groups {
            if grp.title.to_lowercase().contains(&q_lower) {
                return self.auto_play_album(&grp.id).await;
            }
        }

        // 3. Try matching track title or work
        for grp in &groups {
            for (idx, track) in grp.tracks.iter().enumerate() {
                let title = track_display_title(track).to_lowercase();
                let work = first_field(track, "work").unwrap_or_default().to_lowercase();
                if title.contains(&q_lower) || (!work.is_empty() && work.contains(&q_lower)) {
                    let requests: Vec<PlayTrackRequest> =
                        grp.tracks.iter().map(create_play_request).collect();
                    if idx == 0 {
                        return self.replace_queue(requests, true).await;
                    } else {
                        self.replace_queue(requests, false).await?;
                        return self.play_queue_index(idx).await;
                    }
                }
            }
        }

        // 4. Fallback: resume playback if queue exists
        self.play().await
    }

    /// Get full now-playing information for automotive dashboard display
    pub async fn auto_get_now_playing(&self) -> Option<AutoNowPlaying> {
        let snapshot = self.playback.snapshot().await;
        let current = snapshot.current_track?;
        let is_playing = snapshot.status == aria_domain::PlaybackStatus::Playing;

        let library = self.library.snapshot().await;
        let track_info = library.tracks.iter().find(|t| t.id == current.id);

        let album = track_info
            .and_then(|t| first_field(t, "album"))
            .unwrap_or("Aria")
            .to_string();
        let album_id = track_info.map(|t| album_id_for_track(t));
        let artwork_path = track_info.and_then(|t| t.album_art_path.clone());

        let spec = track_info
            .map(|t| {
                let fmt = t.audio.format.to_uppercase();
                match (t.audio.sample_rate, t.audio.bit_depth) {
                    (Some(sr), Some(bd)) => format!("{fmt} {}kHz / {bd}-bit", sr / 1000),
                    (Some(sr), None) => format!("{fmt} {}kHz", sr / 1000),
                    _ => fmt,
                }
            })
            .unwrap_or_else(|| "Audio".into());

        Some(AutoNowPlaying {
            title: current.title,
            artist: current.subtitle,
            album,
            spec,
            position_ms: snapshot.position_ms,
            duration_ms: current.duration_ms,
            is_playing,
            track_id: Some(current.id),
            album_id,
            artwork_path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_domain::AudioPropertiesSnapshot;
    use std::collections::BTreeMap;

    fn make_test_track(
        id: &str,
        path: &str,
        album: &str,
        title: &str,
        composer: &str,
        conductor: &str,
        disc: u32,
        track_num: u32,
        duration_ms: u64,
    ) -> ScannedTrack {
        let mut mapped = BTreeMap::new();
        mapped.insert("album".into(), vec![album.into()]);
        mapped.insert("title".into(), vec![title.into()]);
        mapped.insert("composer".into(), vec![composer.into()]);
        mapped.insert("conductor".into(), vec![conductor.into()]);
        mapped.insert("disc_number".into(), vec![disc.to_string()]);
        mapped.insert("track_number".into(), vec![track_num.to_string()]);

        ScannedTrack {
            id: id.into(),
            path: path.into(),
            file_name: format!("{id}.flac"),
            album_art_path: Some(format!("/art/{album}.jpg")),
            audio: AudioPropertiesSnapshot {
                format: "flac".into(),
                duration_ms,
                sample_rate: Some(96000),
                bit_depth: Some(24),
                channels: Some(2),
            },
            raw_tags: BTreeMap::new(),
            mapped_fields: mapped,
        }
    }

    #[test]
    fn test_group_library_albums_and_sort_tracks() {
        let t1 = make_test_track("t1", "C:/music/b5/01.flac", "Beethoven: Symphony No. 5", "I. Allegro", "Beethoven", "Karajan", 1, 1, 450000);
        let t2 = make_test_track("t2", "C:/music/b5/02.flac", "Beethoven: Symphony No. 5", "II. Andante", "Beethoven", "Karajan", 1, 2, 550000);
        let t3 = make_test_track("t3", "C:/music/bach/01.flac", "Bach: Goldberg Variations", "Aria", "J.S. Bach", "Gould", 1, 1, 180000);

        let groups = group_library_albums(&[t2, t3, t1]);
        assert_eq!(groups.len(), 2);

        // Sorted alphabetically: "Bach: Goldberg Variations" comes before "Beethoven: Symphony No. 5"
        assert_eq!(groups[0].title, "Bach: Goldberg Variations");
        assert_eq!(groups[1].title, "Beethoven: Symphony No. 5");

        // Within Beethoven, tracks are sorted by track_num (t1 then t2)
        assert_eq!(groups[1].tracks[0].id, "t1");
        assert_eq!(groups[1].tracks[1].id, "t2");
        assert_eq!(groups[1].total_duration_ms, 1000000);
    }

    #[test]
    fn test_track_display_title_and_subtitle() {
        let mut track = make_test_track("t1", "C:/music/b5/01.flac", "Beethoven: Symphony No. 5", "Allegro con brio", "Ludwig van Beethoven", "Herbert von Karajan", 1, 1, 450000);
        track.mapped_fields.insert("work".into(), vec!["Symphony No. 5 in C minor, Op. 67".into()]);
        track.mapped_fields.insert("movement".into(), vec!["I. Allegro con brio".into()]);
        track.mapped_fields.insert("ensemble".into(), vec!["Berliner Philharmoniker".into()]);

        let title = track_display_title(&track);
        assert_eq!(title, "Symphony No. 5 in C minor, Op. 67: I. Allegro con brio");

        let subtitle = track_display_subtitle(&track);
        assert_eq!(subtitle, "Ludwig van Beethoven • Herbert von Karajan / Berliner Philharmoniker");
    }
}
