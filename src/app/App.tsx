import { useEffect, useMemo, useRef, useState } from 'react';
import { AlbumPane } from '../features/albums/AlbumPane';
import { LibraryPane } from '../features/library/LibraryPane';
import {
  tracksForAlbum,
  buildAlbumCards,
  buildAlbumTrackRequests,
  buildPlaylistTrackRequests,
  buildPlayTrackRequest,
  directoryForAlbum,
} from '../features/library/view-models';
import {
  DEFAULT_FIELD_MAPPING_FORMAT,
  type FieldMappingFormat,
  normalizeFieldMappingFormat,
  selectInitialFieldMappingFormat,
} from '../lib/field-mapping-presets';
import { PlayerBar } from '../features/playback/PlayerBar';
import { PlaylistPickerDialog } from '../features/playlists/PlaylistPickerDialog';
import { PlaylistPane } from '../features/playlists/PlaylistPane';
import { PlaylistImportDialog } from '../features/playlists/PlaylistImportDialog';
import { QueuePane } from '../features/queue/QueuePane';
import { RemoteUploadDialog } from '../features/remote/RemoteUploadDialog';
import { RemotePlaylistUploadDialog } from '../features/remote/RemotePlaylistUploadDialog';
import { RemoteUploadFailureDialog } from '../features/remote/RemoteUploadFailureDialog';
import { SettingsPane } from '../features/settings/SettingsPane';
import { TrackPane } from '../features/tracks/TrackPane';
import {
  addLibraryRoot,
  addToQueue,
  addTracksToPlaylist,
  bootstrapApp,
  clearLibrary,
  createPlaylist,
  deletePlaylist,
  editTrackTags,
  exportFieldToTag,
  exportPlaylistM3u,
  listOutputDevices,
  refreshOutputDevices,
  listenToAppEvents,
  nextTrack,
  openDirectory,
  pickDirectory,
  play,
  playQueueIndex,
  playTrack,
  previousTrack,
  regeneratePlaylistIcon,
  reportDebugMessage,
  replaceQueue,
  renamePlaylist,
  removeLibraryRoot,
  removeTracksFromPlaylist,
  reorderPlaylistTracks,
  reorderPlaylists,
  restoreQueueOrder,
  seek,
  setCatalogRules,
  setFieldMappings,
  showInExplorer,
  startLibraryScan,
  stop,
  shuffleQueue,
  updateAlbumTrackTableSettings,
  updatePlaylistTrackTableSettings,
  updateTheme,
  updatePlaybackPreferences,
  updateTrackTableSettings,
  pickPlaylistFile,
  syncAllRemoteTargets,
} from '../lib/aria';
import { RefreshIcon } from '../components/RefreshIcon';
import {
  AlbumIcon,
  AriaAppLogo,
  LibraryIcon,
  PlaylistIcon,
  QueueIcon,
  SettingsIcon,
  SidebarToggleIcon,
  TracksIcon,
} from '../components/Icons';
import {
  applyUiPreferences,
  loadUiPreferences,
  saveUiPreferences,
  type UiPreferences,
} from '../lib/ui-preferences';
import type {
  AppBootstrap,
  AppEvent,
  CatalogRule,
  LibraryFieldMapping,
  OutputDeviceSnapshot,
  PlaybackPreferences,
  Playlist,
  PlaylistSnapshot,
  RemoteSyncProgressEvent,
  ScannedTrack,
  ThemePreference,
  TrackTagEditUpdate,
  TrackTableSettings,
  UploadFailedItem,
  UploadProgressEvent,
} from '../types/aria';

type PaneKey = 'library' | 'album' | 'tracks' | 'playlist' | 'queue' | 'settings';

const paneMeta: Record<PaneKey, { label: string }> = {
  library: { label: 'Library' },
  album: { label: 'Album' },
  tracks: { label: 'Tracks' },
  playlist: { label: 'Playlists' },
  queue: { label: 'Queue' },
  settings: { label: 'Settings' },
};

function applyEvent(current: AppBootstrap | null, event: AppEvent): AppBootstrap | null {
  if (!current) {
    return current;
  }

  switch (event.topic) {
    case 'library':
      if (event.payload.kind === 'snapshot_changed') {
        return { ...current, library: event.payload.payload };
      }
      return current;
    case 'playback':
      if (event.payload.kind === 'snapshot_changed') {
        return { ...current, playback: event.payload.payload };
      }
      return current;
    case 'playlists':
      return { ...current, playlists: event.payload.payload };
    case 'remote':
      if (event.payload.kind === 'targets_changed') {
        return { ...current, remoteTargets: event.payload.payload };
      }
      return current;
    case 'settings':
      return { ...current, settings: event.payload };
    default:
      return current;
  }
}

function resolveTheme(theme: ThemePreference, prefersDark: boolean): 'light' | 'dark' {
  if (theme === 'system') {
    return prefersDark ? 'dark' : 'light';
  }

  return theme;
}

function getPaneIcon(pane: PaneKey) {
  switch (pane) {
    case 'library':
      return <LibraryIcon size={17} />;
    case 'album':
      return <AlbumIcon size={17} />;
    case 'tracks':
      return <TracksIcon size={17} />;
    case 'playlist':
      return <PlaylistIcon size={17} />;
    case 'queue':
      return <QueueIcon size={17} />;
    case 'settings':
      return <SettingsIcon size={17} />;
  }
}

export function App() {
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [uiPrefs, setUiPrefs] = useState<UiPreferences>(() => loadUiPreferences());
  const [activePane, setActivePane] = useState<PaneKey>('library');
  const [selectedAlbumId, setSelectedAlbumId] = useState<string | null>(null);
  const [selectedPlaylistId, setSelectedPlaylistId] = useState<string | null>(null);
  const [highlightTrackId, setHighlightTrackId] = useState<string | null>(null);
  const [sessionExportTags, setSessionExportTags] = useState<string[]>([]);
  const [draftMappings, setDraftMappings] = useState<LibraryFieldMapping[]>([]);
  const [draftCatalogRules, setDraftCatalogRules] = useState<CatalogRule[]>([]);
  const [outputDevices, setOutputDevices] = useState<OutputDeviceSnapshot[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [isErrorExpanded, setIsErrorExpanded] = useState(false);
  const [errorCopied, setErrorCopied] = useState(false);
  const playbackPreferencesRequestId = useRef(0);
  const [playlistPickerState, setPlaylistPickerState] = useState<{
    trackIds: string[];
    suggestedName: string;
  } | null>(null);
  const [playlistImportPath, setPlaylistImportPath] = useState<string | null>(null);
  const [uploadAlbumTitle, setUploadAlbumTitle] = useState<string | null>(null);
  const [uploadPlaylist, setUploadPlaylist] = useState<Playlist | null>(null);
  const [activeUploadProgress, setActiveUploadProgress] = useState<UploadProgressEvent | null>(null);
  const [uploadStatusMessage, setUploadStatusMessage] = useState<string | null>(null);
  const [uploadFailedItems, setUploadFailedItems] = useState<UploadFailedItem[] | null>(null);
  const [isSyncingRemote, setIsSyncingRemote] = useState(false);
  const [activeSyncProgress, setActiveSyncProgress] = useState<RemoteSyncProgressEvent | null>(null);
  const [syncStatusMessage, setSyncStatusMessage] = useState<string | null>(null);

  const [selectedMappingFormat, setSelectedMappingFormat] = useState<FieldMappingFormat>(
    DEFAULT_FIELD_MAPPING_FORMAT,
  );
  useEffect(() => {
    // Native webview context menus should never appear unless we add a custom handler.
    const suppressNativeContextMenu = (event: MouseEvent) => {
      event.preventDefault();
    };

    window.addEventListener('contextmenu', suppressNativeContextMenu);
    return () => window.removeEventListener('contextmenu', suppressNativeContextMenu);
  }, []);

  useEffect(() => {
    let active = true;

    void bootstrapApp()
      .then((snapshot) => {
        if (active) {
          setBootstrap(snapshot);
          setDraftMappings(snapshot.library.fieldMappings);
          setDraftCatalogRules(snapshot.library.catalogRules);
          setSelectedMappingFormat(
            selectInitialFieldMappingFormat(snapshot.library.fieldMappings),
          );
        }
      })
      .catch((reason) => {
        if (active) {
          setError(String(reason));
        }
      });

    void listOutputDevices()
      .then((devices) => {
        if (active) {
          setOutputDevices(devices);
        }
      })
      .catch((reason) => {
        if (active) {
          setError(String(reason));
        }
      });

    const handleAudioDevicesChanged = () => {
      void refreshOutputDevices()
        .then((devices) => {
          if (active) {
            setOutputDevices(devices);
          }
        })
        .catch(console.error);
    };

    window.addEventListener('aria-audio-devices-changed', handleAudioDevicesChanged);

    const unlistenPromise = listenToAppEvents((event) => {
      if (
        event.topic === 'playback' &&
        event.payload.kind === 'output_devices_changed'
      ) {
        setOutputDevices(event.payload.payload);
        return;
      }

      if (event.topic === 'remote') {
        if (event.payload.kind === 'upload_progress') {
          setActiveSyncProgress(null);
          setActiveUploadProgress(event.payload.payload);
        } else if (event.payload.kind === 'upload_completed') {
          setActiveUploadProgress(null);
          const completed = event.payload.payload;
          if (completed.failedItems && completed.failedItems.length > 0) {
            setUploadFailedItems(completed.failedItems);
            if (completed.success) {
              setUploadStatusMessage(`Uploaded "${completed.albumTitle}" with ${completed.failedItems.length} failure(s)`);
              setTimeout(() => setUploadStatusMessage(null), 5000);
            } else {
              setError(`Upload incomplete: ${completed.failedItems.length} track(s) failed`);
            }
          } else if (completed.success) {
            if (completed.newTracksCount === 0 && (completed.alreadySyncedTracksCount ?? 0) > 0) {
              setUploadStatusMessage(`${completed.albumTitle} is up to date (all ${completed.alreadySyncedTracksCount} tracks already on remote).`);
            } else {
              setUploadStatusMessage(`Uploaded "${completed.albumTitle}" successfully!`);
            }
            setTimeout(() => setUploadStatusMessage(null), 5000);
          } else {
            setError(`Upload error: ${completed.errorMessage ?? 'Unknown failure'}`);
          }
        } else if (event.payload.kind === 'sync_progress') {
          if (event.payload.payload.status === 'completed') {
            setActiveSyncProgress(null);
          } else {
            setActiveSyncProgress(event.payload.payload);
          }
        } else if (event.payload.kind === 'sync_completed') {
          setActiveSyncProgress(null);
          const completed = event.payload.payload;
          if (completed.success) {
            if (completed.newAlbumsCount > 0 || completed.newTracksCount > 0) {
              setSyncStatusMessage(
                `Synced from ${completed.targetName}: ${completed.newAlbumsCount} new album(s) (${completed.newTracksCount} track(s))`
              );
            } else {
              setSyncStatusMessage(`Remote sync complete (${completed.targetName}): library is up to date.`);
            }
            setTimeout(() => setSyncStatusMessage(null), 5000);
          } else {
            setError(`Remote sync error (${completed.targetName}): ${completed.errorMessage ?? 'Unknown failure'}`);
          }
        }
      }

      setBootstrap((current) => applyEvent(current, event));
    });

    return () => {
      active = false;
      window.removeEventListener('aria-audio-devices-changed', handleAudioDevicesChanged);
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (!import.meta.env.DEV) {
      return undefined;
    }

    const handleError = (event: ErrorEvent) => {
      void reportDebugMessage(
        'window.error',
        event.error ?? event.message ?? 'Unknown window error',
      );
    };

    const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
      void reportDebugMessage('window.unhandledrejection', event.reason);
    };

    window.addEventListener('error', handleError);
    window.addEventListener('unhandledrejection', handleUnhandledRejection);

    return () => {
      window.removeEventListener('error', handleError);
      window.removeEventListener('unhandledrejection', handleUnhandledRejection);
    };
  }, []);

  const albumCards = useMemo(
    () => buildAlbumCards(bootstrap?.library.tracks ?? []),
    [bootstrap?.library.tracks],
  );

  const currentTrackDetails = useMemo(() => {
    const currentTrackId = bootstrap?.playback.currentTrack?.id;
    if (!bootstrap || !currentTrackId) {
      return null;
    }

    return (
      bootstrap.library.tracks.find((track) => track.id === currentTrackId) ?? null
    );
  }, [bootstrap]);

  useEffect(() => {
    if (!selectedAlbumId) {
      return;
    }

    if (!albumCards.some((album) => album.id === selectedAlbumId)) {
      setSelectedAlbumId(albumCards[0]?.id ?? null);
    }
  }, [albumCards, selectedAlbumId]);

  useEffect(() => {
    if (!bootstrap) {
      return;
    }

    if (
      selectedPlaylistId &&
      !bootstrap.playlists.playlists.some((playlist) => playlist.id === selectedPlaylistId)
    ) {
      setSelectedPlaylistId(bootstrap.playlists.playlists[0]?.id ?? null);
    }
  }, [bootstrap, selectedPlaylistId]);

  useEffect(() => {
    const root = document.documentElement;
    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const themePreference = bootstrap?.settings.theme ?? 'dark';

    const applyResolvedTheme = () => {
      const resolvedTheme = resolveTheme(themePreference, mediaQuery.matches);
      root.dataset.theme = resolvedTheme;
      root.style.colorScheme = resolvedTheme;
    };

    applyResolvedTheme();

    if (themePreference !== 'system') {
      return undefined;
    }

    const handleChange = () => applyResolvedTheme();
    mediaQuery.addEventListener('change', handleChange);

    return () => mediaQuery.removeEventListener('change', handleChange);
  }, [bootstrap?.settings.theme]);

  const remainingQueueCount = useMemo(() => {
    if (!bootstrap) {
      return 0;
    }

    if (bootstrap.playback.queue.length === 0) {
      return 0;
    }

    if (bootstrap.playback.currentQueueIndex === null) {
      return bootstrap.playback.queue.length;
    }

    return Math.max(
      bootstrap.playback.queue.length - bootstrap.playback.currentQueueIndex,
      0,
    );
  }, [bootstrap]);

  useEffect(() => {
    applyUiPreferences(uiPrefs);
  }, [uiPrefs]);

  function handleUiPrefsChange(nextPrefs: UiPreferences) {
    setUiPrefs(nextPrefs);
    saveUiPreferences(nextPrefs);
    applyUiPreferences(nextPrefs);
  }

  function handleToggleSidebar() {
    handleUiPrefsChange({
      ...uiPrefs,
      sidebarCollapsed: !uiPrefs.sidebarCollapsed,
    });
  }

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement ||
        event.target instanceof HTMLSelectElement
      ) {
        return;
      }

      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey) {
        if (event.key === '1') {
          event.preventDefault();
          setActivePane('library');
        } else if (event.key === '2') {
          event.preventDefault();
          setActivePane('album');
        } else if (event.key === '3') {
          event.preventDefault();
          setActivePane('tracks');
        } else if (event.key === '4') {
          event.preventDefault();
          setActivePane('playlist');
        } else if (event.key === '5') {
          event.preventDefault();
          setActivePane('queue');
        } else if (event.key === '6') {
          event.preventDefault();
          setActivePane('settings');
        } else if (event.key === 'b' || event.key === 'B') {
          event.preventDefault();
          handleToggleSidebar();
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [uiPrefs]);

  function handleOpenAlbum(albumId: string) {
    setSelectedAlbumId(albumId);
    setActivePane('album');
  }

  function handleOpenTrack(trackId: string) {
    setHighlightTrackId(trackId);
    setActivePane('tracks');
  }

  async function handleAddDirectory() {
    if (!bootstrap || bootstrap.library.isScanning) {
      return;
    }

    try {
      const selectedPath = await pickDirectory();
      if (!selectedPath) {
        return;
      }

      if (bootstrap.library.roots.some((root) => libraryPathsEqual(root.path, selectedPath))) {
        window.alert(`This directory is already in the library:\n\n${selectedPath}`);
        return;
      }

      const library = await addLibraryRoot(selectedPath);
      setBootstrap((current) =>
        current ? { ...current, library } : current,
      );
      await startLibraryScan();
      setError(null);
    } catch (reason) {
      const message = String(reason);
      if (message.includes('already exists')) {
        window.alert('That directory is already cataloged in Aria.');
      }
      setError(message);
    }
  }

  async function handleRemoveRoot(path: string) {
    try {
      const library = await removeLibraryRoot(path);
      setBootstrap((current) =>
        current ? { ...current, library } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleRescanAll() {
    try {
      await startLibraryScan();
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleRefreshFromRemote() {
    const enabledTargets = (bootstrap?.remoteTargets ?? []).filter((t) => t.isEnabled);
    if (enabledTargets.length === 0) {
      setActivePane('settings');
      setError('No enabled remote storage targets found. Please configure or enable one in Settings > Remote Storage.');
      return;
    }

    setIsSyncingRemote(true);
    setError(null);
    setSyncStatusMessage(null);
    try {
      const results = await syncAllRemoteTargets();
      const totalAlbums = results.reduce((acc, r) => acc + r.newAlbumsCount, 0);
      const totalTracks = results.reduce((acc, r) => acc + r.newTracksCount, 0);
      const failed = results.filter((r) => !r.success);
      if (failed.length > 0) {
        setError(`Sync error: ${failed.map((f) => `${f.targetName}: ${f.errorMessage ?? 'Failed'}`).join(', ')}`);
      } else if (totalAlbums > 0 || totalTracks > 0) {
        setSyncStatusMessage(
          `✓ Synced from remote: ${totalAlbums} new album(s) (${totalTracks} track(s)) imported.`
        );
      } else {
        setSyncStatusMessage('✓ Synced from remote: library is up to date.');
      }
      setTimeout(() => setSyncStatusMessage(null), 5000);
    } catch (err) {
      setError(`Remote refresh failed: ${String(err)}`);
    } finally {
      setIsSyncingRemote(false);
    }
  }

  async function handleClearLibrary() {
    if (!bootstrap) {
      return;
    }

    const shouldClear = window.confirm(
      'Clear the library?\n\nThis removes all indexed tracks and library directories from Aria.',
    );

    if (!shouldClear) {
      return;
    }

    try {
      const library = await clearLibrary();
      const playback = await replaceQueue([], false);
      setBootstrap((current) =>
        current
          ? { ...current, library, playback, playlists: { playlists: [] } }
          : current,
      );
      setSelectedAlbumId(null);
      setSelectedPlaylistId(null);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePlay() {
    try {
      const playback = await play();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleStop() {
    try {
      const playback = await stop();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePreviousTrack() {
    try {
      const playback = await previousTrack();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleNextTrack() {
    try {
      const playback = await nextTrack();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleSeek(positionMs: number) {
    try {
      const playback = await seek(positionMs);
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePlayTrack(track: ScannedTrack) {
    try {
      const playback = await playTrack(buildPlayTrackRequest(track));
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePlayTracks(tracksToPlay: ScannedTrack[]) {
    if (tracksToPlay.length === 0) {
      return;
    }

    if (tracksToPlay.length === 1) {
      await handlePlayTrack(tracksToPlay[0]);
      return;
    }

    if (!bootstrap) {
      return;
    }

    const selectedIds = new Set(tracksToPlay.map((track) => track.id));
    const trackLookup = new Map(
      bootstrap.library.tracks.map((track) => [track.id, track]),
    );
    const requests = [
      ...tracksToPlay.map(buildPlayTrackRequest),
      ...bootstrap.playback.queue
        .map((item) => trackLookup.get(item.id) ?? null)
        .filter((track): track is ScannedTrack => track !== null && !selectedIds.has(track.id))
        .map(buildPlayTrackRequest),
    ];

    try {
      const playback = await replaceQueue(requests, true);
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleReplaceQueueAndPlayTracks(tracksToPlay: ScannedTrack[]) {
    if (tracksToPlay.length === 0) {
      return;
    }

    try {
      const playback = await replaceQueue(
        tracksToPlay.map(buildPlayTrackRequest),
        true,
      );
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAddTracksToQueue(tracksToAdd: ScannedTrack[]) {
    if (tracksToAdd.length === 0) {
      return;
    }

    try {
      const playback = await addToQueue(tracksToAdd.map(buildPlayTrackRequest));
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAddAlbumToQueue(albumId: string) {
    if (!bootstrap) {
      return;
    }

    const requests = buildAlbumTrackRequests(bootstrap.library.tracks, albumId);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await addToQueue(requests);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAddAlbumsToQueue(albumIds: string[]) {
    if (!bootstrap) {
      return;
    }

    const requests = buildAlbumRequestsForSelection(bootstrap.library.tracks, albumIds);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await addToQueue(requests);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleReplaceQueue(albumId: string, startPlaying: boolean) {
    if (!bootstrap) {
      return;
    }

    const requests = buildAlbumTrackRequests(bootstrap.library.tracks, albumId);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await replaceQueue(requests, startPlaying);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      if (startPlaying) {
        setSelectedAlbumId(albumId);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleReplaceQueueForAlbums(
    albumIds: string[],
    startPlaying: boolean,
  ) {
    if (!bootstrap) {
      return;
    }

    const requests = buildAlbumRequestsForSelection(bootstrap.library.tracks, albumIds);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await replaceQueue(requests, startPlaying);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      if (startPlaying && albumIds.length > 0) {
        setSelectedAlbumId(albumIds[0]);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleGoToAlbumDirectory(albumId: string) {
    if (!bootstrap) {
      return;
    }

    const path = directoryForAlbum(bootstrap.library.tracks, albumId);
    if (!path) {
      return;
    }

    try {
      await openDirectory(path);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleGoToAlbumDirectories(albumIds: string[]) {
    if (!bootstrap) {
      return;
    }

    const paths = Array.from(
      new Set(
        albumIds
          .map((albumId) => directoryForAlbum(bootstrap.library.tracks, albumId))
          .filter((path): path is string => Boolean(path)),
      ),
    );

    if (paths.length === 0) {
      return;
    }

    try {
      for (const path of paths) {
        await openDirectory(path);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleShowTrackInExplorer(track: ScannedTrack) {
    try {
      await showInExplorer(track.path);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleExportField(
    tracksToExport: ScannedTrack[],
    fieldKey: string,
    tagName: string,
  ) {
    if (tracksToExport.length === 0) {
      return;
    }

    try {
      const library = await exportFieldToTag({
        trackPaths: tracksToExport.map((track) => track.path),
        fieldKey,
        tagName,
      });
      rememberSessionExportTag(tagName);
      setBootstrap((current) => (current ? { ...current, library } : current));
      setError(null);
    } catch (reason) {
      const message = String(reason);
      setError(message);
      throw reason instanceof Error ? reason : message;
    }
  }

  async function handleEditTrackTags(
    tracksToEdit: ScannedTrack[],
    updates: TrackTagEditUpdate[],
  ) {
    if (tracksToEdit.length === 0 || updates.length === 0) {
      return;
    }

    try {
      const library = await editTrackTags({
        trackPaths: tracksToEdit.map((track) => track.path),
        updates,
      });
      setBootstrap((current) => (current ? { ...current, library } : current));
      setError(null);
    } catch (reason) {
      const message = String(reason);
      setError(message);
      throw reason instanceof Error ? reason : message;
    }
  }

  function rememberSessionExportTag(tagName: string) {
    const normalized = normalizeSessionExportTag(tagName);
    if (!normalized) {
      return;
    }

    setSessionExportTags((current) =>
      current.includes(normalized) ? current : [...current, normalized],
    );
  }

  async function handleThemeChange(theme: ThemePreference) {
    try {
      const settings = await updateTheme(theme);
      setBootstrap((current) =>
        current ? { ...current, settings } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAlbumTrackTableChange(trackTable: TrackTableSettings) {
    try {
      const settings = await updateAlbumTrackTableSettings(trackTable);
      setBootstrap((current) =>
        current ? { ...current, settings } : current,
      );
    } catch (reason) {
      void reportDebugMessage('update_album_track_table_settings', reason);
    }
  }

  async function handlePlaylistTrackTableChange(trackTable: TrackTableSettings) {
    try {
      const settings = await updatePlaylistTrackTableSettings(trackTable);
      setBootstrap((current) =>
        current ? { ...current, settings } : current,
      );
    } catch (reason) {
      void reportDebugMessage('update_playlist_track_table_settings', reason);
    }
  }

  async function handleTrackTableChange(trackTable: TrackTableSettings) {
    try {
      const settings = await updateTrackTableSettings(trackTable);
      setBootstrap((current) =>
        current ? { ...current, settings } : current,
      );
    } catch (reason) {
      void reportDebugMessage('update_track_table_settings', reason);
    }
  }

  async function handlePlaybackPreferencesChange(
    playback: PlaybackPreferences,
  ) {
    const requestId = ++playbackPreferencesRequestId.current;

    try {
      const settings = await updatePlaybackPreferences(playback);
      if (playbackPreferencesRequestId.current === requestId) {
        setBootstrap((current) =>
          current ? { ...current, settings } : current,
        );
        setError(null);
      }
    } catch (reason) {
      if (playbackPreferencesRequestId.current === requestId) {
        setError(String(reason));
      }
    }
  }

  function handleVolumeChange(volume: number) {
    if (!bootstrap) {
      return;
    }

    void handlePlaybackPreferencesChange({
      ...bootstrap.settings.playback,
      volume,
    });
  }

  const favoritesPlaylist = bootstrap?.playlists.playlists.find((p) => p.id === 'favorites');
  const isCurrentTrackFavorited = useMemo(() => {
    if (!currentTrackDetails || !favoritesPlaylist) {
      return false;
    }
    return favoritesPlaylist.trackIds.includes(currentTrackDetails.id);
  }, [currentTrackDetails, favoritesPlaylist]);

  async function handleFavoriteCurrentTrack() {
    if (!currentTrackDetails || !favoritesPlaylist) {
      return;
    }
    try {
      const playlists = await addTracksToPlaylist(favoritesPlaylist.id, [currentTrackDetails.id]);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleUnfavoriteCurrentTrack() {
    if (!currentTrackDetails || !favoritesPlaylist) {
      return;
    }
    try {
      const playlists = await removeTracksFromPlaylist(favoritesPlaylist.id, [currentTrackDetails.id]);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleShuffleQueue() {
    try {
      const playback = await shuffleQueue();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleRestoreQueueOrder() {
    try {
      const playback = await restoreQueueOrder();
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleClearQueue() {
    try {
      const playback = await replaceQueue([], false);
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePlayQueueIndex(index: number) {
    try {
      const playback = await playQueueIndex(index);
      setBootstrap((current) =>
        current ? { ...current, playback } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleSaveMappings() {
    try {
      const library = await setFieldMappings(draftMappings);
      setDraftMappings(library.fieldMappings);
      setBootstrap((current) =>
        current ? { ...current, library } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleSaveCatalogRules() {
    try {
      const library = await setCatalogRules(draftCatalogRules);
      setDraftCatalogRules(library.catalogRules);
      setBootstrap((current) =>
        current ? { ...current, library } : current,
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  function handleAddField(format: FieldMappingFormat) {
    setDraftMappings((current) => [
      ...current,
      {
        format: normalizeFieldMappingFormat(format),
        key: '',
        label: 'New Field',
        tagPriorities: [],
      },
    ]);
  }

  function handleRemoveField(format: FieldMappingFormat, index: number) {
    setDraftMappings((current) => {
      const targetIndex = findFieldMappingIndex(current, format, index);
      if (targetIndex < 0) {
        return current;
      }

      return current.filter((_, currentIndex) => currentIndex !== targetIndex);
    });
  }

  function handleUpdateField(
    format: FieldMappingFormat,
    index: number,
    patch: Partial<LibraryFieldMapping>,
  ) {
    setDraftMappings((current) => {
      const targetIndex = findFieldMappingIndex(current, format, index);
      if (targetIndex < 0) {
        return current;
      }

      return current.map((mapping, currentIndex) =>
        currentIndex === targetIndex
          ? {
              ...mapping,
              ...patch,
              format: patch.format ? normalizeFieldMappingFormat(patch.format) : mapping.format,
            }
          : mapping,
      );
    });
  }

  function handleAddCatalogRule() {
    setDraftCatalogRules((current) => [
      ...current,
      {
        label: '',
        composers: [],
        enabled: true,
      },
    ]);
  }

  function handleRemoveCatalogRule(index: number) {
    setDraftCatalogRules((current) =>
      current.filter((_, currentIndex) => currentIndex !== index),
    );
  }

  function handleUpdateCatalogRule(index: number, patch: Partial<CatalogRule>) {
    setDraftCatalogRules((current) =>
      current.map((rule, currentIndex) =>
        currentIndex === index ? { ...rule, ...patch } : rule,
      ),
    );
  }

  function openPlaylistPicker(trackIds: string[], suggestedName: string) {
    if (trackIds.length === 0) {
      return;
    }

    setPlaylistPickerState({
      trackIds: Array.from(new Set(trackIds)),
      suggestedName,
    });
  }

  function handleAddAlbumToPlaylist(albumId: string) {
    if (!bootstrap) {
      return;
    }

    const tracksForSelection = tracksForAlbum(bootstrap.library.tracks, albumId);
    if (tracksForSelection.length === 0) {
      return;
    }

    openPlaylistPicker(
      tracksForSelection.map((track) => track.id),
      tracksForSelection[0]?.mappedFields.album?.[0] ?? 'New playlist',
    );
  }

  function handleAddAlbumsToPlaylist(albumIds: string[]) {
    if (!bootstrap) {
      return;
    }

    const tracksForSelection = albumIds.flatMap((albumId) =>
      tracksForAlbum(bootstrap.library.tracks, albumId),
    );

    if (tracksForSelection.length === 0) {
      return;
    }

    const suggestedName =
      albumIds.length === 1
        ? tracksForSelection[0]?.mappedFields.album?.[0] ?? 'New playlist'
        : `Playlist from ${albumIds.length} albums`;

    openPlaylistPicker(
      tracksForSelection.map((track) => track.id),
      suggestedName,
    );
  }

  function handleAddTracksToPlaylist(tracksToAdd: ScannedTrack[]) {
    if (tracksToAdd.length === 0) {
      return;
    }

    openPlaylistPicker(
      tracksToAdd.map((track) => track.id),
      tracksToAdd.length === 1
        ? tracksToAdd[0].mappedFields.album?.[0] ||
            tracksToAdd[0].mappedFields.title?.[0] ||
            tracksToAdd[0].fileName
        : 'New playlist',
    );
  }

  async function handleCreatePlaylist(name: string) {
    if (!playlistPickerState) {
      return;
    }

    try {
      const playlists = await createPlaylist(name, playlistPickerState.trackIds);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      const created = playlists.playlists[playlists.playlists.length - 1] ?? null;
      setSelectedPlaylistId(created?.id ?? null);
      setPlaylistPickerState(null);
      setActivePane('playlist');
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAddTracksToExistingPlaylist(playlistId: string) {
    if (!playlistPickerState) {
      return;
    }

    try {
      const playlists = await addTracksToPlaylist(playlistId, playlistPickerState.trackIds);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setSelectedPlaylistId(playlistId);
      setPlaylistPickerState(null);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleAddPlaylistToQueue(playlistId: string) {
    if (!bootstrap) {
      return;
    }

    const playlist = bootstrap.playlists.playlists.find((item) => item.id === playlistId);
    if (!playlist) {
      return;
    }

    const requests = buildPlaylistTrackRequests(bootstrap.library.tracks, playlist);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await addToQueue(requests);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handlePlayPlaylist(playlistId: string) {
    if (!bootstrap) {
      return;
    }

    const playlist = bootstrap.playlists.playlists.find((item) => item.id === playlistId);
    if (!playlist) {
      return;
    }

    const requests = buildPlaylistTrackRequests(bootstrap.library.tracks, playlist);
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await replaceQueue(requests, true);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setSelectedPlaylistId(playlistId);
      setActivePane('playlist');
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleShufflePlayPlaylist(playlistId: string) {
    if (!bootstrap) {
      return;
    }

    const playlist = bootstrap.playlists.playlists.find((item) => item.id === playlistId);
    if (!playlist) {
      return;
    }

    const requests = shuffleTrackRequests(
      buildPlaylistTrackRequests(bootstrap.library.tracks, playlist),
    );
    if (requests.length === 0) {
      return;
    }

    try {
      const playback = await replaceQueue(requests, true);
      setBootstrap((current) => (current ? { ...current, playback } : current));
      setSelectedPlaylistId(playlistId);
      setActivePane('playlist');
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleRenamePlaylist(playlistId: string, name: string) {
    try {
      const playlists = await renamePlaylist(playlistId, name);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setSelectedPlaylistId(playlistId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleDeletePlaylist(playlistId: string) {
    try {
      const playlists = await deletePlaylist(playlistId);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      if (selectedPlaylistId === playlistId) {
        setSelectedPlaylistId(playlists.playlists[0]?.id ?? null);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleRegeneratePlaylistIcon(playlistId: string) {
    try {
      const playlists = await regeneratePlaylistIcon(playlistId);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setSelectedPlaylistId(playlistId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleExportPlaylist(playlistId: string) {
    try {
      await exportPlaylistM3u(playlistId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleImportPlaylist() {
    try {
      const selectedPath = await pickPlaylistFile();
      if (selectedPath) {
        setPlaylistImportPath(selectedPath);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  function handleImportSuccess(playlistsSnapshot: PlaylistSnapshot) {
    setBootstrap((current) => (current ? { ...current, playlists: playlistsSnapshot } : current));
    const imported = playlistsSnapshot.playlists[playlistsSnapshot.playlists.length - 1];
    if (imported) {
      setSelectedPlaylistId(imported.id);
      setActivePane('playlist');
    }
    setError(null);
  }

  async function handleRemoveTracksFromPlaylist(
    playlistId: string,
    trackIds: string[],
  ) {
    try {
      const playlists = await removeTracksFromPlaylist(playlistId, trackIds);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setSelectedPlaylistId(playlistId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleReorderPlaylistTracks(
    playlistId: string,
    trackIds: string[],
  ) {
    try {
      const playlists = await reorderPlaylistTracks(playlistId, trackIds);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setSelectedPlaylistId(playlistId);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function handleReorderPlaylists(playlistIds: string[]) {
    try {
      const playlists = await reorderPlaylists(playlistIds);
      setBootstrap((current) => (current ? { ...current, playlists } : current));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  if (!bootstrap) {
    return (
      <main className="app-shell app-shell--loading">
        <div className="loading-card">
          <p className="eyebrow">Aria</p>
          <h1>Bootstrapping desktop shell...</h1>
          <p>{error ?? 'Waiting for the Rust core to become ready.'}</p>
        </div>
      </main>
    );
  }

  return (
    <main className={`app-shell app-shell--${uiPrefs.navStyle}`}>
      <div className="app-shell__body">
        {uiPrefs.navStyle === 'sidebar' ? (
          <aside
            aria-label="App Navigation"
            className={`app-sidebar${uiPrefs.sidebarCollapsed ? ' app-sidebar--collapsed' : ''}`}
          >
            <div className="app-sidebar__header">
              <div
                className="app-sidebar__brand"
                onClick={() => setActivePane('library')}
                role="button"
                style={{ cursor: 'pointer' }}
                tabIndex={0}
                title="Aria — Hi-Fi Classical"
              >
                <span className="app-sidebar__logo">
                  <AriaAppLogo size={28} />
                </span>
                {!uiPrefs.sidebarCollapsed ? (
                  <div className="app-sidebar__brand-text">
                    <span className="app-sidebar__title">Aria</span>
                    <span className="app-sidebar__tagline">Hi-Fi Classical</span>
                  </div>
                ) : null}
              </div>
              <button
                aria-label={uiPrefs.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
                className="ghost-button app-sidebar__collapse-toggle"
                onClick={handleToggleSidebar}
                title={uiPrefs.sidebarCollapsed ? 'Expand sidebar (Ctrl+B)' : 'Collapse sidebar (Ctrl+B)'}
                type="button"
              >
                <SidebarToggleIcon size={16} />
              </button>
            </div>

            <nav className="app-sidebar__nav">
              {(Object.keys(paneMeta) as PaneKey[]).map((pane) => {
                const isActive = activePane === pane;
                return (
                  <button
                    aria-current={isActive ? 'page' : undefined}
                    className={`app-sidebar__item${isActive ? ' app-sidebar__item--active' : ''}`}
                    key={pane}
                    onClick={() => setActivePane(pane)}
                    title={paneMeta[pane].label}
                    type="button"
                  >
                    <span className="app-sidebar__item-icon">{getPaneIcon(pane)}</span>
                    {!uiPrefs.sidebarCollapsed ? (
                      <span className="app-sidebar__item-label">{paneMeta[pane].label}</span>
                    ) : null}
                    {pane === 'queue' && remainingQueueCount > 0 ? (
                      <span className="app-sidebar__item-badge">{remainingQueueCount}</span>
                    ) : null}
                  </button>
                );
              })}
            </nav>

            <div className="app-sidebar__footer">
              <button
                className="ghost-button app-sidebar__sync-button"
                disabled={isSyncingRemote}
                onClick={() => void handleRefreshFromRemote()}
                title="Refresh and download library metadata and tracks from remote storage"
                type="button"
              >
                <RefreshIcon className={isSyncingRemote ? 'aria-spinning' : ''} />
                {!uiPrefs.sidebarCollapsed ? (
                  <span>{isSyncingRemote ? 'Syncing...' : 'Sync Remote'}</span>
                ) : null}
              </button>
            </div>
          </aside>
        ) : null}

        <section className="workspace">
          <div className="workspace__topbar">
            {uiPrefs.navStyle === 'topbar' ? (
              <div className="workspace__brand-and-tabs">
                <div
                  className="app-sidebar__brand"
                  onClick={() => setActivePane('library')}
                  role="button"
                  style={{ cursor: 'pointer' }}
                  tabIndex={0}
                  title="Aria — Hi-Fi Classical"
                >
                  <span className="app-sidebar__logo">
                    <AriaAppLogo size={26} />
                  </span>
                  <div className="app-sidebar__brand-text">
                    <span className="app-sidebar__title" style={{ fontSize: '0.9rem' }}>Aria</span>
                  </div>
                </div>
                <nav className="pane-tabs">
                  {(Object.keys(paneMeta) as PaneKey[]).map((pane) => (
                    <button
                      className={activePane === pane ? 'pane-tabs__active' : 'ghost-button'}
                      key={pane}
                      onClick={() => setActivePane(pane)}
                      type="button"
                    >
                      <span className="pane-tabs__icon">{getPaneIcon(pane)}</span>
                      <span>
                        {pane === 'queue'
                          ? `${paneMeta[pane].label} (${remainingQueueCount})`
                          : paneMeta[pane].label}
                      </span>
                    </button>
                  ))}
                </nav>
              </div>
            ) : (
              <div className="workspace__heading">
                <h1 className="workspace__pane-title">{paneMeta[activePane].label}</h1>
              </div>
            )}

            <div className="workspace__status">
              {uiPrefs.navStyle === 'topbar' ? (
                <button
                  className="ghost-button workspace__sync-button"
                  disabled={isSyncingRemote}
                  onClick={() => void handleRefreshFromRemote()}
                  title="Refresh and download library metadata and tracks from remote storage"
                  type="button"
                >
                  <RefreshIcon className={isSyncingRemote ? 'aria-spinning' : ''} />
                  <span>{isSyncingRemote ? 'Refreshing...' : 'Refresh Remote'}</span>
                </button>
              ) : null}
              {error ? (
                <div
                  className={`device-chip workspace__status-chip workspace__status-chip--error ${
                    isErrorExpanded ? 'workspace__status-chip--expanded' : ''
                  }`}
                  onClick={() => setIsErrorExpanded((prev) => !prev)}
                  title={error}
                  role="alert"
                >
                  <span>{error}</span>
                  <button
                    aria-label="Copy error"
                    className="ghost-button workspace__status-chip-btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      if (navigator.clipboard?.writeText) {
                        navigator.clipboard.writeText(error).then(() => {
                          setErrorCopied(true);
                          setTimeout(() => setErrorCopied(false), 2000);
                        });
                      }
                    }}
                    title="Copy full error message"
                    type="button"
                  >
                    {errorCopied ? 'Copied' : 'Copy'}
                  </button>
                  <button
                    aria-label="Dismiss error"
                    className="ghost-button workspace__status-chip-btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      setError(null);
                      setIsErrorExpanded(false);
                      setActiveUploadProgress(null);
                    }}
                    title="Dismiss"
                    type="button"
                  >
                    ×
                  </button>
                </div>
              ) : activeUploadProgress ? (
                <div
                  className="device-chip workspace__status-chip workspace__status-chip--active"
                  title={`Uploading: ${activeUploadProgress.albumTitle} (${activeUploadProgress.currentFile})`}
                >
                  <RefreshIcon className="aria-spinning" />
                  <span>
                    {(() => {
                      const completed = Math.min(activeUploadProgress.completedFiles, activeUploadProgress.totalFiles);
                      const total = activeUploadProgress.totalFiles;
                      if (activeUploadProgress.status === 'checking_remote') {
                        return `${activeUploadProgress.albumTitle}: Checking remote...`;
                      }
                      if (activeUploadProgress.status === 'already_synced') {
                        return `${activeUploadProgress.albumTitle}: Already synced`;
                      }
                      if (activeUploadProgress.status === 'uploading_art') {
                        return `${activeUploadProgress.albumTitle}: Uploading cover art`;
                      }
                      if (activeUploadProgress.status === 'writing_manifest') {
                        return `${activeUploadProgress.albumTitle}: Writing manifest`;
                      }
                      if (activeUploadProgress.status === 'updating_master_index') {
                        return `${activeUploadProgress.albumTitle}: Updating remote index`;
                      }
                      if (
                        activeUploadProgress.status === 'uploading_playlist_manifest' ||
                        activeUploadProgress.status === 'uploading_playlist_m3u8' ||
                        activeUploadProgress.status === 'updating_playlist_index'
                      ) {
                        return `Saving remote playlist files...`;
                      }
                      if (activeUploadProgress.status === 'uploading_track') {
                        return `${activeUploadProgress.albumTitle}: Uploading (${completed}/${total})`;
                      }
                      if (activeUploadProgress.status === 'track_uploaded') {
                        return `${activeUploadProgress.albumTitle}: Uploaded ${completed}/${total}`;
                      }
                      return `${activeUploadProgress.albumTitle}: ${completed}/${total}`;
                    })()}
                  </span>
                </div>
              ) : activeSyncProgress ? (
                <div
                  className="device-chip workspace__status-chip workspace__status-chip--active"
                  title={activeSyncProgress.currentAlbum ? `Current: ${activeSyncProgress.currentAlbum}` : undefined}
                >
                  <RefreshIcon className="aria-spinning" />
                  <span>
                    {(() => {
                      const { status, completedAlbums, totalAlbums, currentAlbum } = activeSyncProgress;
                      if (status === 'checking_index') {
                        return 'Checking remote library index...';
                      }
                      if (status === 'saving_database') {
                        return currentAlbum || 'Saving to database...';
                      }
                      if (status === 'updating_library') {
                        return currentAlbum || 'Updating local library catalog...';
                      }
                      if (status === 'syncing_playlists') {
                        return currentAlbum || 'Syncing remote playlists...';
                      }
                      if (status === 'albums_fetched') {
                        return currentAlbum || 'Processing synced albums...';
                      }
                      if (totalAlbums > 0 && completedAlbums < totalAlbums) {
                        return `Syncing: ${completedAlbums}/${totalAlbums} albums`;
                      }
                      if (currentAlbum) {
                        return `Syncing: ${currentAlbum}`;
                      }
                      return totalAlbums > 0 ? `Syncing: ${totalAlbums} albums` : 'Syncing remote storage...';
                    })()}
                  </span>
                </div>
              ) : uploadStatusMessage ? (
                <div
                  className="device-chip workspace__status-chip workspace__status-chip--success"
                >
                  <span>{uploadStatusMessage}</span>
                </div>
              ) : syncStatusMessage ? (
                <div
                  className="device-chip workspace__status-chip workspace__status-chip--success"
                >
                  <span>{syncStatusMessage}</span>
                </div>
              ) : null}
            </div>
          </div>

          <section className="workspace__content">
            {activePane === 'library' ? (
              <LibraryPane
                gridSize={uiPrefs.gridSize}
                isSyncingRemote={isSyncingRemote}
                onAddToPlaylist={handleAddAlbumsToPlaylist}
                onAddToQueue={handleAddAlbumsToQueue}
                onGoToDirectory={handleGoToAlbumDirectories}
                onGridSizeChange={(gridSize) => handleUiPrefsChange({ ...uiPrefs, gridSize })}
                onOpenAlbum={handleOpenAlbum}
                onPlayAlbum={(albumIds) => handleReplaceQueueForAlbums(albumIds, true)}
                onRefreshRemote={handleRefreshFromRemote}
                onReplaceQueue={(albumIds) =>
                  handleReplaceQueueForAlbums(albumIds, false)}
                onUploadAlbum={(albumTitle) => setUploadAlbumTitle(albumTitle)}
                selectedAlbumId={selectedAlbumId}
                tracks={bootstrap.library.tracks}
              />
            ) : null}

            {activePane === 'album' ? (
              <AlbumPane
                mappings={bootstrap.library.fieldMappings}
                onAddAlbumToPlaylist={handleAddAlbumToPlaylist}
                onAddTracksToPlaylist={handleAddTracksToPlaylist}
                onAddTracksToQueue={handleAddTracksToQueue}
                onAddToQueue={handleAddAlbumToQueue}
                onBackToLibrary={() => setActivePane('library')}
                onExportField={handleExportField}
                onGoToDirectory={handleGoToAlbumDirectory}
                onPlayAlbum={(albumId) => handleReplaceQueue(albumId, true)}
                onPlayTracks={handlePlayTracks}
                onRememberExportTag={rememberSessionExportTag}
                onReplaceQueue={(albumId) => handleReplaceQueue(albumId, false)}
                onShowInExplorer={handleShowTrackInExplorer}
                onTrackTableChange={handleAlbumTrackTableChange}
                onUploadAlbum={(albumTitle) => setUploadAlbumTitle(albumTitle)}
                selectedAlbumId={selectedAlbumId}
                sessionExportTags={sessionExportTags}
                settings={bootstrap.settings.albumTrackTable}
                tracks={bootstrap.library.tracks}
              />
            ) : null}

            <TrackPane
              highlightTrackId={highlightTrackId}
              isActive={activePane === 'tracks'}
              mappings={bootstrap.library.fieldMappings}
              onAddAlbumToPlaylist={handleAddAlbumToPlaylist}
              onAddAlbumToQueue={handleAddAlbumToQueue}
              onAddToPlaylist={handleAddTracksToPlaylist}
              onAddToQueue={handleAddTracksToQueue}
              onClearHighlightTrack={() => setHighlightTrackId(null)}
              onEditTrackTags={handleEditTrackTags}
              onExportField={handleExportField}
              onGoToDirectory={handleGoToAlbumDirectory}
              onOpenAlbum={handleOpenAlbum}
              onPlayAlbum={(albumId) => handleReplaceQueue(albumId, true)}
              onPlayTracks={handlePlayTracks}
              onRememberExportTag={rememberSessionExportTag}
              onReplaceQueue={(albumId) => handleReplaceQueue(albumId, false)}
              onReplaceQueueAndPlayTracks={handleReplaceQueueAndPlayTracks}
              onShowInExplorer={handleShowTrackInExplorer}
              onTrackTableChange={handleTrackTableChange}
              onUploadAlbum={(albumTitle) => setUploadAlbumTitle(albumTitle)}
              sessionExportTags={sessionExportTags}
              settings={bootstrap.settings.trackTable}
              tracks={bootstrap.library.tracks}
            />

            {activePane === 'playlist' ? (
              <PlaylistPane
                gridSize={uiPrefs.gridSize}
                onAddPlaylistToQueue={handleAddPlaylistToQueue}
                onDeletePlaylist={handleDeletePlaylist}
                onExportPlaylist={handleExportPlaylist}
                onGridSizeChange={(gridSize) => handleUiPrefsChange({ ...uiPrefs, gridSize })}
                onImportPlaylist={handleImportPlaylist}
                onOpenAlbum={handleOpenAlbum}
                onPlayPlaylist={handlePlayPlaylist}
                onPlayTracks={handlePlayTracks}
                onRegeneratePlaylistIcon={handleRegeneratePlaylistIcon}
                onRemoveTracks={handleRemoveTracksFromPlaylist}
                onReorderTracks={handleReorderPlaylistTracks}
                onReorderPlaylists={handleReorderPlaylists}
                onRenamePlaylist={handleRenamePlaylist}
                onSelectPlaylist={setSelectedPlaylistId}
                onShufflePlayPlaylist={handleShufflePlayPlaylist}
                onTrackTableChange={handlePlaylistTrackTableChange}
                onUploadPlaylist={(playlist) => setUploadPlaylist(playlist)}
                playlists={bootstrap.playlists.playlists}
                selectedPlaylistId={selectedPlaylistId}
                settings={bootstrap.settings.playlistTrackTable}
                tracks={bootstrap.library.tracks}
              />
            ) : null}

            {activePane === 'queue' ? (
              <QueuePane
                onClearQueue={handleClearQueue}
                onOpenAlbum={handleOpenAlbum}
                onOpenTrack={handleOpenTrack}
                onPlayQueueIndex={handlePlayQueueIndex}
                onRestoreOrder={handleRestoreQueueOrder}
                onShuffle={handleShuffleQueue}
                playback={bootstrap.playback}
                tracks={bootstrap.library.tracks}
              />
            ) : null}

            {activePane === 'settings' ? (
              <SettingsPane
                currentOutputDevice={bootstrap.playback.outputDevice}
                draftCatalogRules={draftCatalogRules}
                draftMappings={draftMappings}
                library={bootstrap.library}
                onAddCatalogRule={handleAddCatalogRule}
                onAddDirectory={handleAddDirectory}
                onAddField={handleAddField}
                onClearLibrary={handleClearLibrary}
                onPlaybackPreferencesChange={handlePlaybackPreferencesChange}
                onRemoveCatalogRule={handleRemoveCatalogRule}
                onRemoveField={handleRemoveField}
                onRemoveRoot={handleRemoveRoot}
                onRemoteTargetsChange={(targets) =>
                  setBootstrap((current) => (current ? { ...current, remoteTargets: targets } : current))}
                onRescanAll={handleRescanAll}
                onSaveCatalogRules={handleSaveCatalogRules}
                onSaveMappings={handleSaveMappings}
                onSelectMappingFormat={setSelectedMappingFormat}
                onThemeChange={handleThemeChange}
                onUiPrefsChange={handleUiPrefsChange}
                onUpdateCatalogRule={handleUpdateCatalogRule}
                onUpdateField={handleUpdateField}
                outputDevices={outputDevices}
                remoteTargets={bootstrap.remoteTargets ?? []}
                selectedMappingFormat={selectedMappingFormat}
                settings={bootstrap.settings}
                uiPrefs={uiPrefs}
              />
            ) : null}
          </section>
        </section>
      </div>

      <PlayerBar
        currentTrack={currentTrackDetails}
        onNext={handleNextTrack}
        onPlay={handlePlay}
        onStop={handleStop}
        onPrevious={handlePreviousTrack}
        onSeek={handleSeek}
        onVolumeChange={handleVolumeChange}
        playback={bootstrap.playback}
        volume={bootstrap.settings.playback.volume}
        isFavorited={isCurrentTrackFavorited}
        onFavorite={handleFavoriteCurrentTrack}
        onUnfavorite={handleUnfavoriteCurrentTrack}
      />

      {playlistPickerState ? (
        <PlaylistPickerDialog
          onAddToExisting={handleAddTracksToExistingPlaylist}
          onClose={() => setPlaylistPickerState(null)}
          onCreate={handleCreatePlaylist}
          playlists={bootstrap.playlists.playlists}
          suggestedName={playlistPickerState.suggestedName}
          trackCount={playlistPickerState.trackIds.length}
        />
      ) : null}

      {playlistImportPath ? (
        <PlaylistImportDialog
          filePath={playlistImportPath}
          isOpen={true}
          onClose={() => setPlaylistImportPath(null)}
          onImportSuccess={handleImportSuccess}
        />
      ) : null}

      {uploadAlbumTitle ? (
        <RemoteUploadDialog
          albumTitle={uploadAlbumTitle}
          targets={bootstrap.remoteTargets ?? []}
          onClose={() => setUploadAlbumTitle(null)}
          onUploadStarted={(targetName, title) => {
            setError(null);
            setActiveSyncProgress(null);
            setActiveUploadProgress(null);
            setUploadStatusMessage(`Starting upload of "${title}" to ${targetName}...`);
            setTimeout(() => setUploadStatusMessage(null), 4000);
          }}
        />
      ) : null}

      {uploadPlaylist ? (
        <RemotePlaylistUploadDialog
          onClose={() => setUploadPlaylist(null)}
          onUploadStarted={(targetName, playlistName) => {
            setError(null);
            setActiveSyncProgress(null);
            setActiveUploadProgress(null);
            setUploadStatusMessage(`Starting upload of playlist "${playlistName}" to ${targetName}...`);
            setTimeout(() => setUploadStatusMessage(null), 4000);
          }}
          playlist={uploadPlaylist}
          targets={bootstrap.remoteTargets ?? []}
          tracks={bootstrap.library.tracks}
        />
      ) : null}

      {uploadFailedItems && uploadFailedItems.length > 0 ? (
        <RemoteUploadFailureDialog
          failedItems={uploadFailedItems}
          onClose={() => setUploadFailedItems(null)}
        />
      ) : null}
    </main>
  );
}

function normalizeSessionExportTag(tagName: string): string {
  return tagName.trim().toUpperCase();
}

function findFieldMappingIndex(
  mappings: LibraryFieldMapping[],
  format: FieldMappingFormat,
  index: number,
): number {
  const normalizedFormat = normalizeFieldMappingFormat(format);
  let seen = 0;

  for (let currentIndex = 0; currentIndex < mappings.length; currentIndex += 1) {
    if (normalizeFieldMappingFormat(mappings[currentIndex].format) !== normalizedFormat) {
      continue;
    }

    if (seen === index) {
      return currentIndex;
    }

    seen += 1;
  }

  return -1;
}

function shuffleTrackRequests<T>(items: T[]): T[] {
  const shuffled = [...items];
  for (let index = shuffled.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(Math.random() * (index + 1));
    [shuffled[index], shuffled[swapIndex]] = [shuffled[swapIndex], shuffled[index]];
  }
  return shuffled;
}

function normalizeLibraryPath(path: string) {
  const trimmed = path.trim();
  if (!trimmed) {
    return '';
  }
  if (trimmed.length <= 3) {
    return trimmed.toLowerCase();
  }
  return trimmed.replace(/[\\/]+$/, '').toLowerCase();
}

function libraryPathsEqual(left: string, right: string) {
  return normalizeLibraryPath(left) === normalizeLibraryPath(right);
}

function buildAlbumRequestsForSelection(
  tracks: ScannedTrack[],
  albumIds: string[],
) {
  return albumIds.flatMap((albumId) => buildAlbumTrackRequests(tracks, albumId));
}
