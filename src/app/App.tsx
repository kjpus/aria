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
import { PlayerBar } from '../features/playback/PlayerBar';
import { PlaylistPickerDialog } from '../features/playlists/PlaylistPickerDialog';
import { PlaylistPane } from '../features/playlists/PlaylistPane';
import { QueuePane } from '../features/queue/QueuePane';
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
  exportPlaylistM3u,
  listOutputDevices,
  listenToAppEvents,
  nextTrack,
  openDirectory,
  pause,
  pickDirectory,
  play,
  playTrack,
  previousTrack,
  regeneratePlaylistIcon,
  reportDebugMessage,
  replaceQueue,
  renamePlaylist,
  removeLibraryRoot,
  removeTracksFromPlaylist,
  restoreQueueOrder,
  seek,
  setCatalogRules,
  setFieldMappings,
  showInExplorer,
  startLibraryScan,
  shuffleQueue,
  updateAlbumTrackTableSettings,
  updatePlaylistTrackTableSettings,
  updateTheme,
  updatePlaybackPreferences,
  updateTrackTableSettings,
} from '../lib/aria';
import type {
  AppBootstrap,
  AppEvent,
  CatalogRule,
  LibraryFieldMapping,
  OutputDeviceSnapshot,
  PlaybackPreferences,
  ScannedTrack,
  ThemePreference,
  TrackTableSettings,
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

export function App() {
  const [bootstrap, setBootstrap] = useState<AppBootstrap | null>(null);
  const [activePane, setActivePane] = useState<PaneKey>('library');
  const [selectedAlbumId, setSelectedAlbumId] = useState<string | null>(null);
  const [selectedPlaylistId, setSelectedPlaylistId] = useState<string | null>(null);
  const [draftMappings, setDraftMappings] = useState<LibraryFieldMapping[]>([]);
  const [draftCatalogRules, setDraftCatalogRules] = useState<CatalogRule[]>([]);
  const [outputDevices, setOutputDevices] = useState<OutputDeviceSnapshot[]>([]);
  const [error, setError] = useState<string | null>(null);
  const playbackPreferencesRequestId = useRef(0);
  const [playlistPickerState, setPlaylistPickerState] = useState<{
    trackIds: string[];
    suggestedName: string;
  } | null>(null);

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

    const unlistenPromise = listenToAppEvents((event) => {
      if (
        event.topic === 'playback' &&
        event.payload.kind === 'output_devices_changed'
      ) {
        setOutputDevices(event.payload.payload);
        return;
      }

      setBootstrap((current) => applyEvent(current, event));
    });

    return () => {
      active = false;
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

  function handleOpenAlbum(albumId: string) {
    setSelectedAlbumId(albumId);
    setActivePane('album');
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

  async function handlePause() {
    try {
      const playback = await pause();
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

  function handleAddField() {
    setDraftMappings((current) => [
      ...current,
      {
        key: '',
        label: 'New Field',
        tagPriorities: [],
      },
    ]);
  }

  function handleRemoveField(index: number) {
    setDraftMappings((current) => current.filter((_, currentIndex) => currentIndex !== index));
  }

  function handleUpdateField(index: number, patch: Partial<LibraryFieldMapping>) {
    setDraftMappings((current) =>
      current.map((mapping, currentIndex) =>
        currentIndex === index ? { ...mapping, ...patch } : mapping,
      ),
    );
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
    <main className="app-shell">
      <section className="workspace">
        <div className="workspace__topbar">
          <nav className="pane-tabs">
            {(Object.keys(paneMeta) as PaneKey[]).map((pane) => (
              <button
                className={activePane === pane ? 'pane-tabs__active' : 'ghost-button'}
                key={pane}
                onClick={() => setActivePane(pane)}
                type="button"
              >
                {pane === 'queue'
                  ? `${paneMeta[pane].label} (${remainingQueueCount})`
                  : paneMeta[pane].label}
              </button>
            ))}
          </nav>
          <div className="workspace__status">
            {error ? <div className="error-banner">{error}</div> : null}
          </div>
        </div>

        <section className="workspace__content">
          {activePane === 'library' ? (
            <LibraryPane
              onAddToPlaylist={handleAddAlbumsToPlaylist}
              onAddToQueue={handleAddAlbumsToQueue}
              onGoToDirectory={handleGoToAlbumDirectories}
              onOpenAlbum={handleOpenAlbum}
              onPlayAlbum={(albumIds) => handleReplaceQueueForAlbums(albumIds, true)}
              onReplaceQueue={(albumIds) =>
                handleReplaceQueueForAlbums(albumIds, false)}
              selectedAlbumId={selectedAlbumId}
              tracks={bootstrap.library.tracks}
            />
          ) : null}

          {activePane === 'album' ? (
            <AlbumPane
              onAddAlbumToPlaylist={handleAddAlbumToPlaylist}
              onAddTracksToPlaylist={handleAddTracksToPlaylist}
              mappings={bootstrap.library.fieldMappings}
              onAddTracksToQueue={handleAddTracksToQueue}
              onAddToQueue={handleAddAlbumToQueue}
              onGoToDirectory={handleGoToAlbumDirectory}
              onPlayAlbum={(albumId) => handleReplaceQueue(albumId, true)}
              onPlayTracks={handlePlayTracks}
              onReplaceQueue={(albumId) => handleReplaceQueue(albumId, false)}
              onShowInExplorer={handleShowTrackInExplorer}
              selectedAlbumId={selectedAlbumId}
              settings={bootstrap.settings.albumTrackTable}
              onTrackTableChange={handleAlbumTrackTableChange}
              tracks={bootstrap.library.tracks}
            />
          ) : null}

          {activePane === 'tracks' ? (
            <TrackPane
              onAddAlbumToPlaylist={handleAddAlbumToPlaylist}
              onAddToPlaylist={handleAddTracksToPlaylist}
              onAddAlbumToQueue={handleAddAlbumToQueue}
              mappings={bootstrap.library.fieldMappings}
              onAddToQueue={handleAddTracksToQueue}
              onGoToDirectory={handleGoToAlbumDirectory}
              onOpenAlbum={handleOpenAlbum}
              onPlayAlbum={(albumId) => handleReplaceQueue(albumId, true)}
              onPlayTracks={handlePlayTracks}
              onReplaceQueue={(albumId) => handleReplaceQueue(albumId, false)}
              onShowInExplorer={handleShowTrackInExplorer}
              onTrackTableChange={handleTrackTableChange}
              settings={bootstrap.settings.trackTable}
              tracks={bootstrap.library.tracks}
            />
          ) : null}

          {activePane === 'playlist' ? (
            <PlaylistPane
              onDeletePlaylist={handleDeletePlaylist}
              onExportPlaylist={handleExportPlaylist}
              onAddPlaylistToQueue={handleAddPlaylistToQueue}
              onOpenAlbum={handleOpenAlbum}
              onPlayPlaylist={handlePlayPlaylist}
              onShufflePlayPlaylist={handleShufflePlayPlaylist}
              onPlayTracks={handlePlayTracks}
              onRemoveTracks={handleRemoveTracksFromPlaylist}
              onRegeneratePlaylistIcon={handleRegeneratePlaylistIcon}
              onRenamePlaylist={handleRenamePlaylist}
              onSelectPlaylist={setSelectedPlaylistId}
              onTrackTableChange={handlePlaylistTrackTableChange}
              playlists={bootstrap.playlists.playlists}
              selectedPlaylistId={selectedPlaylistId}
              settings={bootstrap.settings.playlistTrackTable}
              tracks={bootstrap.library.tracks}
            />
          ) : null}

          {activePane === 'queue' ? (
            <QueuePane
              onClearQueue={handleClearQueue}
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
              onAddCatalogRule={handleAddCatalogRule}
              onAddDirectory={handleAddDirectory}
              library={bootstrap.library}
              outputDevices={outputDevices}
              onAddField={handleAddField}
              onClearLibrary={handleClearLibrary}
              onPlaybackPreferencesChange={handlePlaybackPreferencesChange}
              onRemoveField={handleRemoveField}
              onRemoveRoot={handleRemoveRoot}
              onRemoveCatalogRule={handleRemoveCatalogRule}
              onSaveCatalogRules={handleSaveCatalogRules}
              onSaveMappings={handleSaveMappings}
              onRescanAll={handleRescanAll}
              onThemeChange={handleThemeChange}
              onUpdateCatalogRule={handleUpdateCatalogRule}
              onUpdateField={handleUpdateField}
              settings={bootstrap.settings}
            />
          ) : null}
        </section>
      </section>

      <PlayerBar
        currentTrack={currentTrackDetails}
        onNext={handleNextTrack}
        onPause={handlePause}
        onPlay={handlePlay}
        onPrevious={handlePreviousTrack}
        onSeek={handleSeek}
        onVolumeChange={handleVolumeChange}
        playback={bootstrap.playback}
        volume={bootstrap.settings.playback.volume}
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
    </main>
  );
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
