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
  volume: number;
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
  catalogRules: CatalogRule[];
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

export type PreviewTrack = {
  title: string;
  path: string;
  trackId: string | null;
};

export type PlaylistImportPreview = {
  filePath: string;
  name: string;
  codepage: number;
  systemDefaultCodepage: number;
  tracks: PreviewTrack[];
};

export type ScanProgress = {
  phase: string;
  processedFiles: number;
  discoveredFiles: number;
  failedFiles: number;
};

export type LibraryFieldMapping = {
  format: string;
  key: string;
  label: string;
  tagPriorities: string[];
};

export type FieldExportRequest = {
  trackPaths: string[];
  fieldKey: string;
  tagName: string;
};

export type TrackTagEditUpdate = {
  tagName: string;
  values: string[];
};

export type TrackTagEditRequest = {
  trackPaths: string[];
  updates: TrackTagEditUpdate[];
};

export type CatalogRule = {
  label: string;
  composers: string[];
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

export type RemoteBackendType = 'google_drive' | 'filesystem' | 'web_dav' | 'smb';

export type RemoteTarget = {
  id: string;
  name: string;
  backendType: RemoteBackendType;
  storageLimitBytes: number | null;
  isEnabled: boolean;
  configJson: string;
};

export type StorageUsage = {
  usedBytes: number;
  totalBytes: number | null;
  freeBytes: number | null;
};

export type StorageStatus = {
  isConnected: boolean;
  message: string | null;
  storageUsage: StorageUsage | null;
};

export type RemoteSyncedItem = {
  itemId: string;
  remoteTargetId: string;
  itemType: string;
  remotePath: string;
  remoteFileId: string | null;
  sizeBytes: number;
  checksum: string | null;
  syncStatus: string;
  lastSyncedAt: string;
};

export type UploadProgressEvent = {
  targetId: string;
  albumId: string;
  albumTitle: string;
  currentFile: string;
  currentFileBytes: number;
  currentFileTotalBytes: number;
  totalFiles: number;
  completedFiles: number;
  totalBytes: number;
  completedBytes: number;
  status: string;
};

export type UploadCompletedEvent = {
  targetId: string;
  albumId: string;
  albumTitle: string;
  success: boolean;
  errorMessage: string | null;
};

export type RemoteCacheStatus = {
  cacheDir: string;
  totalCachedTracks: number;
  totalCachedBytes: number;
  windowSize: number;
};

export type CachePrefetchEvent = {
  trackId: string;
  title: string;
  bytesDownloaded: number;
  totalBytes: number;
  isCompleted: boolean;
  errorMessage: string | null;
};

export type GoogleAuthCompletedEvent = {
  success: boolean;
  errorMessage: string | null;
};

export type RemoteEvent =
  | {
      kind: 'targets_changed';
      payload: RemoteTarget[];
    }
  | {
      kind: 'upload_progress';
      payload: UploadProgressEvent;
    }
  | {
      kind: 'upload_completed';
      payload: UploadCompletedEvent;
    }
  | {
      kind: 'cache_prefetch_progress';
      payload: CachePrefetchEvent;
    }
  | {
      kind: 'google_auth_completed';
      payload: GoogleAuthCompletedEvent;
    };

export type AppBootstrap = {
  library: LibrarySnapshot;
  playback: PlaybackSnapshot;
  playlists: PlaylistSnapshot;
  settings: SettingsSnapshot;
  remoteTargets?: RemoteTarget[];
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

export type PlaybackEvent =
  | {
      kind: 'snapshot_changed';
      payload: PlaybackSnapshot;
    }
  | {
      kind: 'output_devices_changed';
      payload: OutputDeviceSnapshot[];
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
      topic: 'remote';
      payload: RemoteEvent;
    }
  | {
      topic: 'settings';
      payload: SettingsSnapshot;
    };
