use aria_domain::ScannedTrack;

pub(crate) fn sanitize_playlist_filename(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if cleaned.is_empty() {
        "playlist".into()
    } else {
        cleaned
    }
}

pub(crate) fn clean_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut result = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => {
                result.push(other.as_os_str());
            }
        }
    }
    result
}

pub(crate) fn normalize_for_comparison(path: &std::path::Path) -> String {
    let cleaned = clean_path(path);
    cleaned.to_string_lossy().replace('\\', "/").to_lowercase()
}

pub fn fold_for_matching(s: &str) -> String {
    // 1. Spacing accents (e.g. acute accent '\u{00b4}' / CP1252 0xB4, diaeresis '\u{00a8}', cedilla '\u{00b8}',
    //    or grave accent '`' / 0x60 after a letter) frequently result from legacy ANSI / Windows-1252
    //    WideCharToMultiByte best-fit conversion of Unicode NFD combining marks ('\u{0301}', '\u{0308}', '\u{0327}', '\u{0300}'),
    //    or typing spacing accents in place of accented letters.
    //    Replace them with combining marks so deunicode strips them consistently with NFC/NFD accented letters.
    // 2. Greek beta '\u{03b2}' / '\u{03d0}' is frequently typed or CP437-converted in place of German sharp s 'ß' (U+00DF).
    let mut normalized = String::with_capacity(s.len());
    let mut prev_is_alpha = false;
    for ch in s.chars() {
        match ch {
            '\u{00b4}' => normalized.push('\u{0301}'),
            '\u{00a8}' => normalized.push('\u{0308}'),
            '\u{00b8}' => normalized.push('\u{0327}'),
            '\u{0060}' if prev_is_alpha => normalized.push('\u{0300}'),
            '\u{03b2}' | '\u{03d0}' | 'ß' => normalized.push_str("ss"),
            _ => normalized.push(ch),
        }
        prev_is_alpha = ch.is_alphabetic();
    }
    deunicode::deunicode(&normalized)
}

fn extract_track_number_prefix(filename: &str) -> Option<u32> {
    let trimmed = filename.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let rest = &trimmed[digits.len()..];
        if rest.starts_with('.')
            || rest.starts_with('-')
            || rest.starts_with(' ')
            || rest.starts_with('_')
        {
            return digits.parse::<u32>().ok();
        }
    }
    None
}

pub struct PlaylistTrackMatcher<'a> {
    exact_path: std::collections::HashMap<String, &'a str>,
    folded_path: std::collections::HashMap<String, Vec<&'a str>>,
    by_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
    by_folded_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
    by_parent_folder: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
}

impl<'a> PlaylistTrackMatcher<'a> {
    pub fn new(tracks: &'a [ScannedTrack]) -> Self {
        let mut exact_path = std::collections::HashMap::with_capacity(tracks.len());
        let mut folded_path: std::collections::HashMap<String, Vec<&'a str>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_folded_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_parent_folder: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::new();

        for track in tracks {
            let norm = normalize_for_comparison(std::path::Path::new(&track.path));
            exact_path.insert(norm.clone(), track.id.as_str());

            let folded = fold_for_matching(&norm);
            folded_path.entry(folded).or_default().push(track.id.as_str());

            let track_p = std::path::Path::new(&track.path);
            if let Some(file_name) = track_p.file_name().and_then(|f| f.to_str()) {
                let fn_lower = file_name.to_lowercase();
                by_filename.entry(fn_lower.clone()).or_default().push(track);

                let fn_folded = fold_for_matching(&fn_lower);
                by_folded_filename.entry(fn_folded).or_default().push(track);
            }

            if let Some(parent_name) = track_p
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|f| f.to_str())
            {
                let folder_folded = fold_for_matching(parent_name).to_lowercase();
                by_parent_folder.entry(folder_folded).or_default().push(track);
            }
        }

        Self {
            exact_path,
            folded_path,
            by_filename,
            by_folded_filename,
            by_parent_folder,
        }
    }

    pub fn find_match(&self, resolved: &std::path::Path) -> Option<String> {
        let norm = normalize_for_comparison(resolved);

        // Tier 1: Exact normalized path match
        if let Some(id) = self.exact_path.get(&norm) {
            return Some((*id).to_string());
        }

        // Tier 2: Diacritic / accent / homoglyph-folded path match
        let folded = fold_for_matching(&norm);
        if let Some(ids) = self.folded_path.get(&folded) {
            if ids.len() == 1 {
                return Some(ids[0].to_string());
            }
        }

        // Tier 3: Filename + Parent directory / Unique Filename match
        if let Some(file_name) = resolved.file_name().and_then(|f| f.to_str()) {
            let fn_lower = file_name.to_lowercase();
            let fn_folded = fold_for_matching(&fn_lower);
            let candidates = self
                .by_filename
                .get(&fn_lower)
                .or_else(|| self.by_folded_filename.get(&fn_folded));

            if let Some(candidates) = candidates {
                if candidates.len() == 1 {
                    return Some(candidates[0].id.clone());
                }

                // If multiple candidate tracks share this filename, match on parent folder name
                if let Some(playlist_parent) = resolved
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|f| f.to_str())
                {
                    let folded_parent = fold_for_matching(playlist_parent).to_lowercase();
                    let parent_matches: Vec<&ScannedTrack> = candidates
                        .iter()
                        .copied()
                        .filter(|c| {
                            let track_path = std::path::Path::new(&c.path);
                            track_path
                                .parent()
                                .and_then(|p| p.file_name())
                                .and_then(|f| f.to_str())
                                .map(|name| fold_for_matching(name).to_lowercase() == folded_parent)
                                .unwrap_or(false)
                        })
                        .collect();

                    if parent_matches.len() == 1 {
                        return Some(parent_matches[0].id.clone());
                    }
                }
            }
        }

        // Tier 4: Parent Folder + Track Number Prefix Match
        // Handles cases where track titles in filenames had punctuation or minor differences
        if let Some(playlist_parent) = resolved
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
        {
            let folded_parent = fold_for_matching(playlist_parent).to_lowercase();
            if let Some(folder_tracks) = self.by_parent_folder.get(&folded_parent) {
                if let Some(file_name) = resolved.file_name().and_then(|f| f.to_str()) {
                    if let Some(target_track_num) = extract_track_number_prefix(file_name) {
                        let num_matches: Vec<&ScannedTrack> = folder_tracks
                            .iter()
                            .copied()
                            .filter(|c| {
                                std::path::Path::new(&c.path)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                    .and_then(extract_track_number_prefix)
                                    == Some(target_track_num)
                            })
                            .collect();

                        if num_matches.len() == 1 {
                            return Some(num_matches[0].id.clone());
                        }

                        // If multiple candidate tracks share this track number prefix in the folder
                        // (common in multi-work / compilation albums without disc subdirectories),
                        // disambiguate using token overlap from the filename.
                        if num_matches.len() > 1 {
                            let target_folded = fold_for_matching(file_name).to_lowercase();
                            let target_tokens: std::collections::HashSet<&str> = target_folded
                                .split(|c: char| !c.is_alphanumeric())
                                .filter(|s| !s.is_empty())
                                .collect();

                            let mut best_candidate: Option<(&ScannedTrack, usize)> = None;
                            let mut is_ambiguous = false;

                            for candidate in num_matches {
                                if let Some(cand_name) = std::path::Path::new(&candidate.path)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                {
                                    let cand_folded = fold_for_matching(cand_name).to_lowercase();
                                    let cand_tokens: std::collections::HashSet<&str> = cand_folded
                                        .split(|c: char| !c.is_alphanumeric())
                                        .filter(|s| !s.is_empty())
                                        .collect();

                                    let overlap = target_tokens.intersection(&cand_tokens).count();
                                    match best_candidate {
                                        Some((_, best_score)) if overlap > best_score => {
                                            best_candidate = Some((candidate, overlap));
                                            is_ambiguous = false;
                                        }
                                        Some((_, best_score))
                                            if overlap == best_score && overlap > 0 =>
                                        {
                                            is_ambiguous = true;
                                        }
                                        None if overlap > 0 => {
                                            best_candidate = Some((candidate, overlap));
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if let Some((best_track, _)) = best_candidate {
                                if !is_ambiguous {
                                    return Some(best_track.id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetACP() -> u32;
    fn MultiByteToWideChar(
        CodePage: u32,
        dwFlags: u32,
        lpMultiByteStr: *const u8,
        cbMultiByte: i32,
        lpWideCharStr: *mut u16,
        cchWideChar: i32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
pub fn get_system_default_codepage() -> u32 {
    unsafe { GetACP() }
}

#[cfg(not(target_os = "windows"))]
pub fn get_system_default_codepage() -> u32 {
    65001 // UTF-8 fallback
}

#[cfg(target_os = "windows")]
pub(crate) fn decode_with_codepage(bytes: &[u8], codepage: u32) -> Result<String, String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }

    let input_len = i32::try_from(bytes.len())
        .map_err(|_| "Playlist file exceeds maximum Win32 buffer size".to_string())?;
    unsafe {
        let required_len = MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            std::ptr::null_mut(),
            0,
        );

        if required_len <= 0 {
            return Err("Failed to convert string (length calculation)".to_string());
        }

        let mut buf = vec![0u16; required_len as usize];
        let result = MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            buf.as_mut_ptr(),
            required_len,
        );

        if result <= 0 {
            return Err("Failed to convert string".to_string());
        }

        Ok(String::from_utf16_lossy(&buf))
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn decode_with_codepage(bytes: &[u8], _codepage: u32) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|err| err.to_string())
}

pub(crate) fn resolve_playlist_codepage<'a>(
    extension: &str,
    bytes: &'a [u8],
    codepage: Option<u32>,
    system_cp: u32,
) -> (u32, &'a [u8]) {
    let has_utf8_bom = bytes.starts_with(b"\xEF\xBB\xBF");
    let default_cp = if extension == "m3u8" || has_utf8_bom {
        65001
    } else {
        system_cp
    };
    let selected_cp = match codepage {
        Some(0) | None => default_cp,
        Some(cp) => cp,
    };

    let decode_bytes = if selected_cp == 65001 && has_utf8_bom {
        &bytes[3..]
    } else {
        bytes
    };

    (selected_cp, decode_bytes)
}

pub(crate) fn parse_playlist_content(content: &str, extension: &str) -> Vec<(String, Option<String>)> {
    let mut raw_paths = Vec::new();
    if extension == "pls" {
        let mut files = std::collections::BTreeMap::new();
        let mut titles = std::collections::BTreeMap::new();
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('\u{FEFF}');
            if trimmed.is_empty() {
                continue;
            }
            if let Some(eq_idx) = trimmed.find('=') {
                let key = trimmed[..eq_idx].trim().to_ascii_lowercase();
                let val = trimmed[eq_idx + 1..].trim().to_string();
                if !val.is_empty() {
                    if let Some(num_str) = key.strip_prefix("file") {
                        if let Ok(num) = num_str.parse::<u32>() {
                            files.insert(num, val);
                        }
                    } else if let Some(num_str) = key.strip_prefix("title") {
                        if let Ok(num) = num_str.parse::<u32>() {
                            titles.insert(num, val);
                        }
                    }
                }
            }
        }
        for (num, file_path) in files {
            let title = titles.get(&num).cloned();
            raw_paths.push((file_path, title));
        }
    } else {
        // Default to M3U / M3U8 parsing
        let mut last_extinf = None;
        for line in content.lines() {
            let trimmed = line.trim().trim_start_matches('\u{FEFF}');
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("#EXTINF:") {
                if let Some(comma_idx) = trimmed.find(',') {
                    last_extinf = Some(trimmed[comma_idx + 1..].trim().to_string());
                }
            } else if trimmed.starts_with('#') {
                continue;
            } else {
                raw_paths.push((trimmed.to_string(), last_extinf.take()));
            }
        }
    }
    raw_paths
}
