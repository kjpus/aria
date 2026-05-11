import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { defaultFieldMappings } from './field-mapping-presets';
import { isTauriRuntime } from './runtime';
import type {
  AppBootstrap,
  AppEvent,
  CatalogRule,
  FieldExportRequest,
  LibraryFieldMapping,
  LibrarySnapshot,
  OutputDeviceSnapshot,
  PlaybackPreferences,
  PlayTrackRequest,
  PlaylistSnapshot,
  PlaybackSnapshot,
  SettingsSnapshot,
  ThemePreference,
  TrackTagEditRequest,
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
    fieldMappings: defaultFieldMappings(),
    catalogRules: [
      {
        label: 'BWV',
        composers: ['Johann Sebastian Bach', 'Bach'],
        enabled: true,
      },
      {
        label: 'WAB',
        composers: ['Anton Bruckner', 'Bruckner'],
        enabled: true,
      },
      {
        label: 'K',
        composers: ['Wolfgang Amadeus Mozart', 'Mozart'],
        enabled: true,
      },
      {
        label: 'D',
        composers: ['Franz Schubert', 'Schubert'],
        enabled: true,
      },
      {
        label: 'WoO',
        composers: [],
        enabled: true,
      },
      {
        label: 'Op',
        composers: [],
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
    playlistTrackTable: {
      visibleColumns: [
        'track_number',
        'title',
        'composer',
        'conductor',
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
      volume: 1,
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
  rules: CatalogRule[],
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

export async function exportFieldToTag(
  request: FieldExportRequest,
): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    const requestedPaths = new Set(request.trackPaths);
    const nextTracks = previewBootstrap.library.tracks.map((track) => {
      if (!requestedPaths.has(track.path)) {
        return track;
      }

      const values = [...(track.mappedFields[request.fieldKey] ?? [])];
      const nextRawTags = { ...track.rawTags };

      if (values.length > 0) {
        nextRawTags[request.tagName] = values;
      } else {
        delete nextRawTags[request.tagName];
      }

      return {
        ...track,
        rawTags: nextRawTags,
      };
    });

    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        tracks: nextTracks,
        tagInventory: rebuildPreviewTagInventory(nextTracks),
      },
    };

    return previewBootstrap.library;
  }

  return invoke<LibrarySnapshot>('export_field_to_tag', { request });
}

export async function editTrackTags(
  request: TrackTagEditRequest,
): Promise<LibrarySnapshot> {
  if (!isTauriRuntime) {
    const requestedPaths = new Set(request.trackPaths);
    const nextTracks = previewBootstrap.library.tracks.map((track) => {
      if (!requestedPaths.has(track.path)) {
        return track;
      }

      const nextRawTags = { ...track.rawTags };
      for (const update of request.updates) {
        if (update.values.length > 0) {
          nextRawTags[update.tagName] = [...update.values];
        } else {
          delete nextRawTags[update.tagName];
        }
      }

      return {
        ...track,
        rawTags: nextRawTags,
      };
    });

    previewBootstrap = {
      ...previewBootstrap,
      library: {
        ...previewBootstrap.library,
        tracks: nextTracks,
        tagInventory: rebuildPreviewTagInventory(nextTracks),
      },
    };

    return previewBootstrap.library;
  }

  return invoke<LibrarySnapshot>('edit_track_tags', { request });
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
  const normalizedTrackTable = normalizeTrackTableSettingsPayload(trackTable);

  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        trackTable: normalizedTrackTable,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_track_table_settings', {
    trackTable: normalizedTrackTable,
  });
}

export async function updateAlbumTrackTableSettings(
  albumTrackTable: TrackTableSettings,
): Promise<SettingsSnapshot> {
  const normalizedTrackTable =
    normalizeTrackTableSettingsPayload(albumTrackTable);

  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        albumTrackTable: normalizedTrackTable,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_album_track_table_settings', {
    albumTrackTable: normalizedTrackTable,
  });
}

export async function updatePlaylistTrackTableSettings(
  playlistTrackTable: TrackTableSettings,
): Promise<SettingsSnapshot> {
  const normalizedTrackTable =
    normalizeTrackTableSettingsPayload(playlistTrackTable);

  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        playlistTrackTable: normalizedTrackTable,
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_playlist_track_table_settings', {
    playlistTrackTable: normalizedTrackTable,
  });
}

export async function reportDebugMessage(
  context: string,
  error: unknown,
): Promise<void> {
  if (!import.meta.env.DEV) {
    return;
  }

  const message = `[${context}] ${stringifyDebugError(error)}`;
  console.error(message, error);

  if (!isTauriRuntime) {
    return;
  }

  try {
    await invoke<void>('debug_log', { message });
  } catch (invokeError) {
    console.error('[debug_log]', invokeError);
  }
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
  const normalizedPlayback = normalizePlaybackPreferencesPayload(playback);

  if (!isTauriRuntime) {
    previewBootstrap = {
      ...previewBootstrap,
      settings: {
        ...previewBootstrap.settings,
        playback: normalizedPlayback,
      },
      playback: {
        ...previewBootstrap.playback,
        outputDevice:
          previewOutputDevices.find((device) => device.id === normalizedPlayback.outputDeviceId) ??
          previewOutputDevices[0],
      },
    };
    return previewBootstrap.settings;
  }
  return invoke<SettingsSnapshot>('update_playback_preferences', {
    playback: normalizedPlayback,
  });
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

function stringifyDebugError(error: unknown): string {
  if (error instanceof Error) {
    return error.stack ?? error.message;
  }

  if (typeof error === 'string') {
    return error;
  }

  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

function normalizeTrackTableSettingsPayload(
  trackTable: TrackTableSettings,
): TrackTableSettings {
  return {
    ...trackTable,
    visibleColumns: [...trackTable.visibleColumns],
    columnWidths: Object.fromEntries(
      Object.entries(trackTable.columnWidths).map(([key, value]) => [
        key,
        Math.max(0, Math.round(value)),
      ]),
    ),
    secondarySort: trackTable.secondarySort.map((criterion) => ({ ...criterion })),
  };
}

function normalizePlaybackPreferencesPayload(
  playback: PlaybackPreferences,
): PlaybackPreferences {
  return {
    ...playback,
    volume: clampPlaybackVolume(playback.volume),
  };
}

function clampPlaybackVolume(volume: number): number {
  if (!Number.isFinite(volume)) {
    return 1;
  }

  return Math.min(Math.max(volume, 0), 1);
}

function rebuildPreviewTagInventory(tracks: LibrarySnapshot['tracks']) {
  const inventory = new Map<string, { occurrences: number; exampleValues: string[] }>();

  for (const track of tracks) {
    for (const [tag, values] of Object.entries(track.rawTags)) {
      const existing = inventory.get(tag) ?? {
        occurrences: 0,
        exampleValues: [],
      };
      existing.occurrences += 1;

      for (const value of values) {
        if (existing.exampleValues.length >= 3) {
          break;
        }
        if (!existing.exampleValues.includes(value)) {
          existing.exampleValues.push(value);
        }
      }

      inventory.set(tag, existing);
    }
  }

  return Array.from(inventory.entries())
    .map(([tag, entry]) => ({
      tag,
      occurrences: entry.occurrences,
      exampleValues: entry.exampleValues,
    }))
    .sort(
      (left, right) =>
        right.occurrences - left.occurrences || left.tag.localeCompare(right.tag),
    );
}
