import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { isTauriRuntime } from './runtime';
import type {
  AppBootstrap,
  AppEvent,
  CatalogPatternRule,
  LibraryFieldMapping,
  LibrarySnapshot,
  OutputDeviceSnapshot,
  PlaybackPreferences,
  PlayTrackRequest,
  PlaylistSnapshot,
  PlaybackSnapshot,
  SettingsSnapshot,
  ThemePreference,
  TrackRawTags,
  TrackTableSettings,
} from '../types/aria';

export const APP_EVENT_NAME = 'aria://app-event';

let previewBootstrap: AppBootstrap = {
  library: {
    roots: [
      {
        path: 'C:\\Users\\hongswan\\Music\\Great 50\\Great 50',
        label: 'Great 50',
      },
    ],
    isScanning: false,
    indexedFiles: 603,
    lastScanAt: 'Preview mode',
    fieldMappings: [
      { key: 'album', label: 'Album', tagPriorities: ['ALBUM'] },
      { key: 'title', label: 'Title', tagPriorities: ['TITLE'] },
      { key: 'catalog', label: 'Catalog', tagPriorities: ['CATALOGNUMBER', 'CATALOG'] },
      { key: 'composer', label: 'Composer', tagPriorities: ['COMPOSER'] },
      { key: 'genre', label: 'Genre', tagPriorities: ['GENRE'] },
      { key: 'conductor', label: 'Conductor', tagPriorities: ['CONDUCTOR'] },
      { key: 'ensemble', label: 'Ensemble', tagPriorities: ['ENSEMBLE', 'ORCHESTRA', 'ALBUMARTIST'] },
      { key: 'soloist', label: 'Soloist', tagPriorities: ['PERFORMER', 'ARTIST', 'ALBUMARTIST'] },
      { key: 'year', label: 'Year', tagPriorities: ['DATE', 'YEAR'] },
      { key: 'disk_number', label: 'Disk Number', tagPriorities: ['DISCNUMBER'] },
      { key: 'track_number', label: 'Track Number', tagPriorities: ['TRACKNUMBER'] },
    ],
    catalogRules: [
      {
        label: 'Opus',
        pattern: String.raw`(?i)\b(?:Op\.?|Opus)\s*\d+[A-Za-z]?(?:\s*No\.?\s*\d+)?\b`,
        composers: [],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
      {
        label: 'BWV',
        pattern: String.raw`(?i)\bBWV\s*\d+[A-Za-z]?\b`,
        composers: ['Johann Sebastian Bach', 'Bach'],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
      {
        label: 'WAB',
        pattern: String.raw`(?i)\bWAB\s*\d+[A-Za-z]?\b`,
        composers: ['Anton Bruckner', 'Bruckner'],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
      {
        label: 'K',
        pattern: String.raw`(?i)\bK\.?\s*\d+[A-Za-z]?\b`,
        composers: ['Wolfgang Amadeus Mozart', 'Mozart'],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
      {
        label: 'D',
        pattern: String.raw`(?i)\bD\.?\s*\d+[A-Za-z]?\b`,
        composers: ['Franz Schubert', 'Schubert'],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
      {
        label: 'WoO',
        pattern: String.raw`(?i)\bWoO\s*\d+[A-Za-z]?\b`,
        composers: [],
        sourceTags: ['TITLE', 'WORK', 'ALBUM'],
        enabled: true,
      },
    ],
    tagInventory: [
      { tag: 'TITLE', occurrences: 603, exampleValues: ['Symphony No. 2 in G major'] },
      { tag: 'ALBUM', occurrences: 603, exampleValues: ['Vaughan Williams Symphony No. 2'] },
      { tag: 'COMPOSER', occurrences: 603, exampleValues: ['Ralph Vaughan Williams'] },
      { tag: 'PERFORMER', occurrences: 420, exampleValues: ['Jascha Heifetz', 'Arthur Rubinstein'] },
      { tag: 'CONDUCTOR', occurrences: 278, exampleValues: ['Bernard Haitink'] },
    ],
    tracks: [
      {
        id: 'preview-1',
        path: 'C:\\Users\\hongswan\\Music\\Great 50\\Great 50\\Bernard Haitink - Vaughan Williams Symphony No. 2\\01 - Symphony No. 2.flac',
        fileName: '01 - Symphony No. 2.flac',
        albumArtPath: null,
        audio: {
          format: 'FLAC',
          durationMs: 941000,
          sampleRate: 44100,
          bitDepth: 16,
          channels: 2,
        },
        rawTags: {
          ALBUM: ['Vaughan Williams Symphony No. 2 A London Symphony'],
          TITLE: ['Symphony No. 2 in G major "A London Symphony": I. Lento - Allegro risoluto'],
          COMPOSER: ['Ralph Vaughan Williams'],
          CONDUCTOR: ['Bernard Haitink'],
          GENRE: ['Classical'],
          DATE: ['1988'],
          TRACKNUMBER: ['1'],
        },
        mappedFields: {
          album: ['Vaughan Williams Symphony No. 2 A London Symphony'],
          title: ['Symphony No. 2 in G major "A London Symphony": I. Lento - Allegro risoluto'],
          catalog: [],
          composer: ['Ralph Vaughan Williams'],
          genre: ['Classical'],
          conductor: ['Bernard Haitink'],
          ensemble: [],
          soloist: [],
          year: ['1988'],
          disk_number: [],
          track_number: ['1'],
        },
      },
    ],
  },
  playback: {
    status: 'paused',
    currentTrack: {
      id: 'preview-now-playing',
      title: 'Vaughan Williams — Symphony No. 2 in G major',
      subtitle: 'Bernard Haitink • London Philharmonic Orchestra',
      durationMs: 941000,
    },
    queue: [
      {
        id: 'preview-now-playing',
        title: 'Vaughan Williams — Symphony No. 2 in G major',
        subtitle: 'Bernard Haitink • London Philharmonic Orchestra',
        durationMs: 941000,
      },
    ],
    currentQueueIndex: 0,
    queueDepth: 1,
    positionMs: 223000,
    outputDevice: {
      id: 'system-default',
      name: 'System Default',
      backend: 'preview',
      exclusiveCapable: false,
      isDefault: true,
    },
  },
  playlists: {
    playlists: [
      {
        id: 'preview-playlist-1',
        name: 'Keyboard Essentials',
        collageSeed: 0,
        trackIds: ['preview-1'],
        createdAt: '2026-04-12T10:00:00Z',
      },
    ],
  },
  settings: {
    theme: 'dark',
    accentColor: '#d6b16a',
    trackTable: {
      visibleColumns: [
        'track_number',
        'title',
        'composer',
        'conductor',
        'album',
        'year',
        'format',
      ],
      columnWidths: {
        track_number: 96,
        disk_number: 96,
        year: 110,
        format: 110,
        duration: 110,
        file_name: 220,
        path: 360,
        title: 280,
        album: 260,
        composer: 220,
        conductor: 220,
        ensemble: 220,
        soloist: 220,
      },
      sortKey: 'album',
      sortDirection: 'asc',
      secondarySort: [
        { key: 'track_number', direction: 'asc' },
        { key: 'title', direction: 'asc' },
      ],
    },
    albumTrackTable: {
      visibleColumns: [
        'track_number',
        'title',
        'composer',
        'conductor',
        'ensemble',
        'format',
        'duration',
      ],
      columnWidths: {
        track_number: 96,
        disk_number: 96,
        year: 110,
        format: 110,
        duration: 110,
        file_name: 220,
        path: 360,
        title: 280,
        album: 260,
        composer: 220,
        conductor: 220,
        ensemble: 220,
        soloist: 220,
      },
      sortKey: 'track_number',
      sortDirection: 'asc',
      secondarySort: [{ key: 'title', direction: 'asc' }],
    },
    playback: {
      outputDeviceId: null,
      exclusiveMode: false,
    },
  },
};

const previewOutputDevices: OutputDeviceSnapshot[] = [
  {
    id: 'system-default',
    name: 'System Default',
    backend: 'preview',
    exclusiveCapable: false,
    isDefault: true,
  },
  {
    id: 'preview-dac',
    name: 'USB DAC',
    backend: 'preview',
    exclusiveCapable: false,
    isDefault: false,
  },
];

export async function bootstrapApp(): Promise<AppBootstrap> {
  if (!isTauriRuntime) {
    return previewBootstrap;
  }
  return invoke<AppBootstrap>('bootstrap');
}

export async function addLibraryRoot(path: string): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        roots: [
          ...previewBootstrap.library.roots,
          { path, label: path.split('\\').filter(Boolean).pop() ?? path },
        ],
      },
    };
    return previewBootstrap.library;
  }
  return invoke<LibrarySnapshot>('add_library_root', { path });
}

export async function clearLibrary(): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        roots: [],
        indexedFiles: 0,
        lastScanAt: null,
        tagInventory: [],
        tracks: [],
      },
      playlists: {
        playlists: [],
      },
    };
    return previewBootstrap.library;
  }
  return invoke<LibrarySnapshot>('clear_library');
}

export async function removeLibraryRoot(path: string): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        roots: previewBootstrap.library.roots.filter((root) => root.path !== path),
      },
    };
    return previewBootstrap.library;
  }
  return invoke<LibrarySnapshot>('remove_library_root', { path });
}

export async function startLibraryScan(): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke<void>('start_library_scan');
}

export async function createPlaylist(
  name: string,
  trackIds: string[],
): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    const trimmed = name.trim();
    if (!trimmed) {
      throw new Error('Playlist name cannot be empty');
    }

    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: [
          ...previewBootstrap.playlists.playlists,
          {
            id: `preview-playlist-${Date.now()}`,
            name: trimmed,
            collageSeed: 0,
            trackIds: Array.from(new Set(trackIds)),
            createdAt: new Date().toISOString(),
          },
        ],
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('create_playlist', { name, trackIds });
}

export async function addTracksToPlaylist(
  playlistId: string,
  trackIds: string[],
): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: previewBootstrap.playlists.playlists.map((playlist) =>
          playlist.id === playlistId
            ? {
                ...playlist,
                trackIds: Array.from(new Set([...playlist.trackIds, ...trackIds])),
              }
            : playlist,
        ),
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('add_tracks_to_playlist', {
    playlistId,
    trackIds,
  });
}

export async function renamePlaylist(
  playlistId: string,
  name: string,
): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    const trimmed = name.trim();
    if (!trimmed) {
      throw new Error('Playlist name cannot be empty');
    }

    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: previewBootstrap.playlists.playlists.map((playlist) =>
          playlist.id === playlistId ? { ...playlist, name: trimmed } : playlist,
        ),
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('rename_playlist', { playlistId, name });
}

export async function deletePlaylist(playlistId: string): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: previewBootstrap.playlists.playlists.filter(
          (playlist) => playlist.id !== playlistId,
        ),
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('delete_playlist', { playlistId });
}

export async function regeneratePlaylistIcon(
  playlistId: string,
): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: previewBootstrap.playlists.playlists.map((playlist) =>
          playlist.id === playlistId
            ? { ...playlist, collageSeed: playlist.collageSeed + 1 }
            : playlist,
        ),
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('regenerate_playlist_icon', { playlistId });
}

export async function exportPlaylistM3u(
  playlistId: string,
): Promise<string | null> {
  if (!isTauriRuntime) {
    const playlist = previewBootstrap.playlists.playlists.find(
      (item) => item.id === playlistId,
    );
    return playlist ? `${playlist.name}.m3u` : null;
  }
  return invoke<string | null>('export_playlist_m3u', { playlistId });
}

export async function removeTracksFromPlaylist(
  playlistId: string,
  trackIds: string[],
): Promise<PlaylistSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      playlists: {
        playlists: previewBootstrap.playlists.playlists.map((playlist) =>
          playlist.id === playlistId
            ? {
                ...playlist,
                trackIds: playlist.trackIds.filter((trackId) => !trackIds.includes(trackId)),
              }
            : playlist,
        ),
      },
    };
    return previewBootstrap.playlists;
  }
  return invoke<PlaylistSnapshot>('remove_tracks_from_playlist', { playlistId, trackIds });
}

export async function pickDirectory(): Promise<string | null> {
  if (!isTauriRuntime) {
    return null;
  }
  return invoke<string | null>('pick_directory');
}

export async function setFieldMappings(
  mappings: LibraryFieldMapping[],
): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        fieldMappings: mappings,
      },
    };
    return previewBootstrap.library;
  }
  return invoke<LibrarySnapshot>('set_field_mappings', { mappings });
}

export async function setCatalogRules(
  rules: CatalogPatternRule[],
): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        catalogRules: rules,
      },
    };
    return previewBootstrap.library;
  }
  return invoke<LibrarySnapshot>('set_catalog_rules', { rules });
}

export async function play(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('play');
}

export async function playTrack(
  request: PlayTrackRequest,
): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        status: 'playing',
        currentTrack: request.queueItem,
        queue: [request.queueItem],
        currentQueueIndex: 0,
        queueDepth: 1,
        positionMs: 0,
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('play_track', { request });
}

export async function addToQueue(
  requests: PlayTrackRequest[],
): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const queue = [...previewBootstrap.playback.queue, ...requests.map((request) => request.queueItem)];
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        currentTrack: previewBootstrap.playback.currentTrack ?? queue[0] ?? null,
        queue,
        currentQueueIndex:
          previewBootstrap.playback.currentQueueIndex ?? (queue.length > 0 ? 0 : null),
        queueDepth: queue.length,
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('add_to_queue', { requests });
}

export async function replaceQueue(
  requests: PlayTrackRequest[],
  startPlaying: boolean,
): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const queue = requests.map((request) => request.queueItem);
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        status: startPlaying && queue.length > 0 ? 'playing' : 'stopped',
        currentTrack: queue[0] ?? null,
        queue,
        currentQueueIndex: queue.length > 0 ? 0 : null,
        queueDepth: queue.length,
        positionMs: 0,
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('replace_queue', { requests, startPlaying });
}

export async function pause(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('pause');
}

export async function previousTrack(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const currentIndex = previewBootstrap.playback.currentQueueIndex ?? 0;
    const nextIndex = Math.max(0, currentIndex - 1);
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        status: previewBootstrap.playback.queue[nextIndex] ? 'playing' : 'stopped',
        currentTrack: previewBootstrap.playback.queue[nextIndex] ?? null,
        currentQueueIndex: previewBootstrap.playback.queue[nextIndex] ? nextIndex : null,
        positionMs: 0,
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('previous_track');
}

export async function nextTrack(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const currentIndex = previewBootstrap.playback.currentQueueIndex ?? -1;
    const nextIndex = currentIndex + 1;
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        status: previewBootstrap.playback.queue[nextIndex] ? 'playing' : 'stopped',
        currentTrack: previewBootstrap.playback.queue[nextIndex] ?? null,
        currentQueueIndex: previewBootstrap.playback.queue[nextIndex] ? nextIndex : null,
        positionMs: 0,
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('next_track');
}

export async function shuffleQueue(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const currentIndex = previewBootstrap.playback.currentQueueIndex ?? -1;
    const played = previewBootstrap.playback.queue.slice(0, currentIndex + 1);
    const unplayed = [...previewBootstrap.playback.queue.slice(currentIndex + 1)].sort(() => Math.random() - 0.5);
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        queue: [...played, ...unplayed],
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('shuffle_queue');
}

export async function restoreQueueOrder(): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('restore_queue_order');
}

export async function seek(positionMs: number): Promise<PlaybackSnapshot> {
  if (!isTauriRuntime) {
    const duration = previewBootstrap.playback.currentTrack?.durationMs ?? 0;
    previewBootstrap = {
      ...previewBootstrap,
      playback: {
        ...previewBootstrap.playback,
        positionMs: Math.min(Math.max(0, positionMs), duration),
      },
    };
    return previewBootstrap.playback;
  }
  return invoke<PlaybackSnapshot>('seek', { positionMs });
}

export async function openDirectory(path: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke<void>('open_directory', { path });
}

export async function showInExplorer(path: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke<void>('show_in_explorer', { path });
}

export async function readTrackRawTags(
  path: string,
  fallbackRawTags?: TrackRawTags,
): Promise<TrackRawTags> {
  if (!isTauriRuntime) {
    return fallbackRawTags ?? {};
  }
  return invoke<TrackRawTags>('read_track_raw_tags', { path });
}

export async function updateTheme(
  theme: ThemePreference,
): Promise<SettingsSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        theme,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_theme', { theme });
}

export async function updateTrackTableSettings(
  trackTable: TrackTableSettings,
): Promise<SettingsSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        trackTable,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_track_table_settings', { trackTable });
}

export async function updateAlbumTrackTableSettings(
  albumTrackTable: TrackTableSettings,
): Promise<SettingsSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        albumTrackTable,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_album_track_table_settings', { albumTrackTable });
}

export async function listOutputDevices(): Promise<OutputDeviceSnapshot[]> {
  if (!isTauriRuntime) {
    return previewOutputDevices;
  }
  return invoke<OutputDeviceSnapshot[]>('list_output_devices');
}

export async function updatePlaybackPreferences(
  playback: PlaybackPreferences,
): Promise<SettingsSnapshot> {
  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        playback,
      },
      playback: {
        ...previewBootstrap.playback,
        outputDevice:
          previewOutputDevices.find((device) => device.id === playback.outputDeviceId) ??
          previewOutputDevices[0],
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_playback_preferences', { playback });
}

export async function listenToAppEvents(
  handler: (event: AppEvent) => void,
): Promise<() => void> {
  if (!isTauriRuntime) {
    return () => {
      void handler;
    };
  }
  return listen<AppEvent>(APP_EVENT_NAME, (event) => {
    handler(event.payload);
  });
}
