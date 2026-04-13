export type ThemePreference = 'system' | 'light' | 'dark';
export type TrackSortDirection = 'asc' | 'desc';
export type TrackSortCriterion = {
  key: string;
  direction: TrackSortDirection;
};

export type TrackTableSettings = {
  visibleColumns: string[];
  columnWidths: Record<string, number>;
  sortKey: string;
  sortDirection: TrackSortDirection;
  secondarySort: TrackSortCriterion[];
};

export type PlaybackPreferences = {
  outputDeviceId: string | null;
  exclusiveMode: boolean;
};

export type LibraryRoot = {
  path: string;
  label: string;
};

export type LibrarySnapshot = {
  roots: LibraryRoot[];
  isScanning: boolean;
  indexedFiles: number;
  lastScanAt: string | null;
  fieldMappings: LibraryFieldMapping[];
  catalogRules: CatalogPatternRule[];
  tagInventory: TagInventoryEntry[];
  tracks: ScannedTrack[];
};

export type Playlist = {
  id: string;
  name: string;
  collageSeed: number;
  trackIds: string[];
  createdAt: string | null;
};

export type PlaylistSnapshot = {
  playlists: Playlist[];
};

export type ScanProgress = {
  phase: string;
  processedFiles: number;
  discoveredFiles: number;
  failedFiles: number;
};

export type LibraryFieldMapping = {
  key: string;
  label: string;
  tagPriorities: string[];
};

export type CatalogPatternRule = {
  label: string;
  pattern: string;
  composers: string[];
  sourceTags: string[];
  enabled: boolean;
};

export type TagInventoryEntry = {
  tag: string;
  occurrences: number;
  exampleValues: string[];
};

export type AudioPropertiesSnapshot = {
  format: string;
  durationMs: number;
  sampleRate: number | null;
  bitDepth: number | null;
  channels: number | null;
};

export type ScannedTrack = {
  id: string;
  path: string;
  fileName: string;
  albumArtPath: string | null;
  audio: AudioPropertiesSnapshot;
  rawTags: Record<string, string[]>;
  mappedFields: Record<string, string[]>;
};

export type TrackRawTags = Record<string, string[]>;

export type QueueItem = {
  id: string;
  title: string;
  subtitle: string;
  durationMs: number;
};

export type PlayTrackRequest = {
  path: string;
  queueItem: QueueItem;
};

export type OutputDeviceSnapshot = {
  id: string;
  name: string;
  backend: string;
  exclusiveCapable: boolean;
  isDefault: boolean;
};

export type PlaybackStatus = 'stopped' | 'paused' | 'playing' | 'buffering';

export type PlaybackSnapshot = {
  status: PlaybackStatus;
  currentTrack: QueueItem | null;
  queue: QueueItem[];
  currentQueueIndex: number | null;
  queueDepth: number;
  positionMs: number;
  outputDevice: OutputDeviceSnapshot;
};

export type SettingsSnapshot = {
  theme: ThemePreference;
  accentColor: string;
  trackTable: TrackTableSettings;
  albumTrackTable: TrackTableSettings;
  playlistTrackTable: TrackTableSettings;
  playback: PlaybackPreferences;
};

export type AppBootstrap = {
  library: LibrarySnapshot;
  playback: PlaybackSnapshot;
  playlists: PlaylistSnapshot;
  settings: SettingsSnapshot;
};

export type LibraryEvent =
  | {
      kind: 'snapshot_changed';
      payload: LibrarySnapshot;
    }
  | {
      kind: 'scan_progress';
      payload: ScanProgress;
    };

export type PlaybackEvent = {
  kind: 'snapshot_changed';
  payload: PlaybackSnapshot;
};

export type PlaylistEvent = {
  kind: 'snapshot_changed';
  payload: PlaylistSnapshot;
};

export type AppEvent =
  | {
      topic: 'library';
      payload: LibraryEvent;
    }
  | {
      topic: 'playback';
      payload: PlaybackEvent;
    }
  | {
      topic: 'playlists';
      payload: PlaylistEvent;
    }
  | {
      topic: 'settings';
      payload: SettingsSnapshot;
    };
