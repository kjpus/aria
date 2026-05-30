import type {
  LibraryFieldMapping,
  PlaybackSnapshot,
  PlayTrackRequest,
  Playlist,
  QueueItem,
  ScannedTrack,
} from '../../types/aria';

export type AlbumCardModel = {
  id: string;
  title: string;
  composer: string;
  credit: string;
  year: string;
  artPath: string | null;
  trackCount: number;
};

export type TrackColumn = {
  key: string;
  label: string;
};

export function joinField(track: ScannedTrack, key: string): string {
  return (track.mappedFields[key] ?? []).join(' / ');
}

export function firstField(track: ScannedTrack, key: string): string {
  return track.mappedFields[key]?.[0] ?? '';
}

export function albumTitleForTrack(track: ScannedTrack): string {
  return firstField(track, 'album') || track.fileName;
}

export function albumIdForTrack(track: ScannedTrack): string {
  return albumTitleForTrack(track).trim().toLowerCase() || track.id;
}

export function albumCreditForTrack(track: ScannedTrack): string {
  return joinField(track, 'conductor') || joinField(track, 'soloist');
}

export function buildAlbumCards(tracks: ScannedTrack[]): AlbumCardModel[] {
  const albums = new Map<string, AlbumCardModel>();

  for (const track of tracks) {
    const title = albumTitleForTrack(track);
    const composer = joinField(track, 'composer');
    const credit = albumCreditForTrack(track);
    const year = joinField(track, 'year');
    const id = albumIdForTrack(track);

    const existing = albums.get(id);
    if (existing) {
      existing.trackCount += 1;
      if (!existing.artPath && track.albumArtPath) {
        existing.artPath = track.albumArtPath;
      }
      if (!existing.year && year) {
        existing.year = year;
      }
      existing.composer = mergeAlbumText(existing.composer, composer);
      existing.credit = mergeAlbumText(existing.credit, credit);
      continue;
    }

    albums.set(id, {
      id,
      title,
      composer,
      credit,
      year,
      artPath: track.albumArtPath,
      trackCount: 1,
    });
  }

  return Array.from(albums.values()).sort((left, right) =>
    left.title.localeCompare(right.title),
  );
}

export function buildTrackColumns(mappings: LibraryFieldMapping[]): TrackColumn[] {
  const seenKeys = new Set<string>();
  const mappingColumns: TrackColumn[] = [];

  for (const mapping of mappings) {
    if (mapping.key && mapping.label && !seenKeys.has(mapping.key)) {
      seenKeys.add(mapping.key);
      mappingColumns.push({
        key: mapping.key,
        label: mapping.label,
      });
    }
  }

  return [
    ...mappingColumns,
    { key: 'format', label: 'Format' },
    { key: 'duration', label: 'Duration' },
    { key: 'file_name', label: 'File' },
    { key: 'path', label: 'Path' },
  ];
}

export function getTrackColumnValue(track: ScannedTrack, key: string): string {
  switch (key) {
    case 'format':
      return track.audio.format;
    case 'duration':
      return formatDuration(track.audio.durationMs);
    case 'file_name':
      return track.fileName;
    case 'path':
      return track.path;
    default:
      return joinField(track, key);
  }
}

export function buildAlbumTrackRequests(
  tracks: ScannedTrack[],
  albumId: string,
): PlayTrackRequest[] {
  return tracksForAlbum(tracks, albumId).map(buildPlayTrackRequest);
}

export function buildPlaylistTrackRequests(
  tracks: ScannedTrack[],
  playlist: Playlist,
): PlayTrackRequest[] {
  return tracksForPlaylist(tracks, playlist).map(buildPlayTrackRequest);
}

export function directoryForAlbum(
  tracks: ScannedTrack[],
  albumId: string,
): string | null {
  const [firstTrack] = tracksForAlbum(tracks, albumId);
  if (!firstTrack) {
    return null;
  }

  const lastSeparator = Math.max(
    firstTrack.path.lastIndexOf('\\'),
    firstTrack.path.lastIndexOf('/'),
  );
  return lastSeparator >= 0 ? firstTrack.path.slice(0, lastSeparator) : null;
}

export function sortQueueItems(
  items: QueueItem[],
  mode: 'ordered' | 'shuffled',
): QueueItem[] {
  if (mode === 'ordered') {
    return items;
  }

  return [...items].sort((left, right) => {
    const leftScore = hashTrack(left.id);
    const rightScore = hashTrack(right.id);
    return leftScore - rightScore;
  });
}

export function buildPlayTrackRequest(track: ScannedTrack): PlayTrackRequest {
  const title = firstField(track, 'title') || track.fileName;
  const composer = joinField(track, 'composer');
  const primaryCredit =
    joinField(track, 'conductor') ||
    joinField(track, 'soloist') ||
    joinField(track, 'ensemble');
  const subtitleParts = [composer, primaryCredit].filter(Boolean);

  return {
    path: track.path,
    queueItem: {
      id: track.id,
      title,
      subtitle: subtitleParts.join(' / '),
      durationMs: track.audio.durationMs,
    },
  };
}

export function buildPlayerSubtitle(playback: PlaybackSnapshot): string {
  if (!playback.currentTrack) {
    return 'Choose a track from the library to begin playback.';
  }

  return playback.currentTrack.subtitle;
}

export function formatDuration(durationMs: number): string {
  const totalSeconds = Math.floor(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}:${minutes.toString().padStart(2, '0')}:${seconds
      .toString()
      .padStart(2, '0')}`;
  }

  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export function tracksForAlbum(tracks: ScannedTrack[], albumId: string): ScannedTrack[] {
  return tracks
    .filter((track) => albumIdForTrack(track) === albumId)
    .sort(compareTracksWithinAlbum);
}

export function tracksForPlaylist(
  tracks: ScannedTrack[],
  playlist: Playlist,
): ScannedTrack[] {
  const trackLookup = new Map(tracks.map((track) => [track.id, track]));
  return playlist.trackIds
    .map((trackId) => trackLookup.get(trackId) ?? null)
    .filter((track): track is ScannedTrack => track !== null);
}

function hashTrack(input: string): number {
  let hash = 0;
  for (let index = 0; index < input.length; index += 1) {
    hash = (hash * 31 + input.charCodeAt(index)) % 104729;
  }
  return hash;
}

export function compareTracksWithinAlbum(
  left: ScannedTrack,
  right: ScannedTrack,
): number {
  const leftDiskNumber = numericMappedFieldValue(left, 'disk_number');
  const rightDiskNumber = numericMappedFieldValue(right, 'disk_number');
  const leftTrackNumber = numericMappedFieldValue(left, 'track_number');
  const rightTrackNumber = numericMappedFieldValue(right, 'track_number');

  if (leftDiskNumber !== null && rightDiskNumber !== null) {
    const diskComparison = compareNullableNumbers(leftDiskNumber, rightDiskNumber);
    if (diskComparison !== 0) {
      return diskComparison;
    }
  }

  const trackComparison = compareNullableNumbers(leftTrackNumber, rightTrackNumber);
  if (trackComparison !== 0) {
    return trackComparison;
  }

  return compareTrackTitleOrFileName(left, right);
}

function compareNullableNumbers(
  left: number | null,
  right: number | null,
): number {
  if (left === right) {
    return 0;
  }

  if (left === null) {
    return 1;
  }

  if (right === null) {
    return -1;
  }

  return left - right;
}

function compareTrackTitleOrFileName(
  left: ScannedTrack,
  right: ScannedTrack,
): number {
  const leftValue = firstField(left, 'title') || left.fileName;
  const rightValue = firstField(right, 'title') || right.fileName;

  return leftValue.localeCompare(rightValue, undefined, {
    numeric: true,
    sensitivity: 'base',
  });
}

function numericMappedFieldValue(track: ScannedTrack, key: string): number | null {
  return parseNumericValue(firstField(track, key));
}

function parseNumericValue(value: string): number | null {
  const match = value.match(/\d+/);
  if (!match) {
    return null;
  }

  const parsedValue = Number.parseInt(match[0], 10);
  return Number.isNaN(parsedValue) ? null : parsedValue;
}

function mergeAlbumText(existing: string, next: string): string {
  const values = [...splitDisplayValues(existing), ...splitDisplayValues(next)];
  const deduped: string[] = [];

  for (const value of values) {
    if (
      !deduped.some(
        (entry) => entry.toLocaleLowerCase() === value.toLocaleLowerCase(),
      )
    ) {
      deduped.push(value);
    }
  }

  return deduped.join(' / ');
}

function splitDisplayValues(value: string): string[] {
  return value
    .split('/')
    .map((part) => part.trim())
    .filter(Boolean);
}
