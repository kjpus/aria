import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import { useDeferredValue, useEffect, useMemo, useRef, useState } from 'react';
import { ClearableInput } from '../../components/ClearableInput';
import { EditTrackTagsDialog } from '../../components/EditTrackTagsDialog';
import { ExportFieldDialog } from '../../components/ExportFieldDialog';
import { SectionCard } from '../../components/SectionCard';
import { TrackRawTagsDialog } from '../../components/TrackRawTagsDialog';
import { readTrackRawTags } from '../../lib/aria';
import type {
  LibraryFieldMapping,
  ScannedTrack,
  TrackTagEditUpdate,
  TrackRawTags,
  TrackSortCriterion,
  TrackSortDirection,
  TrackTableSettings,
} from '../../types/aria';
import type { AlbumCardModel } from '../library/view-models';
import {
  albumIdForTrack,
  buildAlbumCards,
  buildTrackColumns,
  compareTracksWithinAlbum,
  getTrackColumnValue,
} from '../library/view-models';
import { HoverScrollText } from '../albums/HoverScrollText';

type TrackPaneProps = {
  isActive: boolean;
  tracks: ScannedTrack[];
  mappings: LibraryFieldMapping[];
  settings: TrackTableSettings;
  onAddAlbumToPlaylist: (albumId: string) => void | Promise<void>;
  onAddAlbumToQueue: (albumId: string) => void | Promise<void>;
  onGoToDirectory: (albumId: string) => void | Promise<void>;
  onPlayAlbum: (albumId: string) => void | Promise<void>;
  onExportField: (
    tracks: ScannedTrack[],
    fieldKey: string,
    tagName: string,
  ) => void | Promise<void>;
  onEditTrackTags: (
    tracks: ScannedTrack[],
    updates: TrackTagEditUpdate[],
  ) => void | Promise<void>;
  onRememberExportTag: (tagName: string) => void;
  onPlayTracks: (tracks: ScannedTrack[]) => void | Promise<void>;
  onReplaceQueueAndPlayTracks: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddToQueue: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddToPlaylist: (tracks: ScannedTrack[]) => void | Promise<void>;
  onOpenAlbum: (albumId: string) => void;
  onReplaceQueue: (albumId: string) => void | Promise<void>;
  sessionExportTags: string[];
  onShowInExplorer: (track: ScannedTrack) => void | Promise<void>;
  onTrackTableChange: (settings: TrackTableSettings) => void;
};

const defaultTrackColumns = [
  'track_number',
  'title',
  'composer',
  'conductor',
  'year',
  'format',
];

const defaultSortKeys = [
  'composer',
  'disk_number',
  'track_number',
  'title',
];

const trackTableAlbumRowHeight = 44;
const trackTableTrackRowHeight = 48;
const trackTableOverscanPx = 480;
const trackTableFallbackViewportHeight = 720;

type DropPosition = 'before' | 'after';

type DropIndicator = {
  key: string;
  position: DropPosition;
};

type ResizeState = {
  key: string;
  partnerKey: string;
  startX: number;
  startWidth: number;
  startPartnerWidth: number;
};

type TrackContextMenuState = {
  x: number;
  y: number;
  trackIds: string[];
  primaryTrackId: string;
};

type AlbumContextMenuState = {
  x: number;
  y: number;
  albumId: string;
};

type AlbumTrackGroup = {
  albumId: string;
  album: AlbumCardModel | null;
  tracks: ScannedTrack[];
};

type TrackTableRow =
  | {
      key: string;
      kind: 'album';
      group: AlbumTrackGroup;
    }
  | {
      key: string;
      kind: 'track';
      albumId: string;
      track: ScannedTrack;
    };

type VirtualizedTrackTable = {
  rows: TrackTableRow[];
  rowOffsets: number[];
  totalHeight: number;
};

export function TrackPane({
  isActive,
  tracks,
  mappings,
  onAddAlbumToPlaylist,
  onAddAlbumToQueue,
  onGoToDirectory,
  onPlayAlbum,
  onExportField,
  onEditTrackTags,
  onRememberExportTag,
  onPlayTracks,
  onReplaceQueueAndPlayTracks,
  onAddToQueue,
  onAddToPlaylist,
  onOpenAlbum,
  onReplaceQueue,
  sessionExportTags,
  onShowInExplorer,
  settings,
  onTrackTableChange,
}: TrackPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const trackTableScrollRef = useRef<HTMLDivElement>(null);
  const [isLayoutDialogOpen, setIsLayoutDialogOpen] = useState(false);
  const [filter, setFilter] = useState('');
  const deferredFilter = useDeferredValue(filter);
  const columns = useMemo(
    () => buildTrackColumns(mappings).filter((column) => column.key !== 'album'),
    [mappings],
  );
  const columnLookup = useMemo(
    () => new Map(columns.map((column) => [column.key, column])),
    [columns],
  );
  const [visibleColumns, setVisibleColumns] = useState<string[]>([]);
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({});
  const [sortCriteria, setSortCriteria] = useState<TrackSortCriterion[]>([]);
  const [expandedAlbumIds, setExpandedAlbumIds] = useState<string[]>([]);
  const [selectedTrackIds, setSelectedTrackIds] = useState<string[]>([]);
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(null);
  const [draggedVisibleColumn, setDraggedVisibleColumn] = useState<string | null>(
    null,
  );
  const [visibleDropIndicator, setVisibleDropIndicator] =
    useState<DropIndicator | null>(null);
  const [resizingColumn, setResizingColumn] = useState<string | null>(null);
  const [albumContextMenu, setAlbumContextMenu] =
    useState<AlbumContextMenuState | null>(null);
  const [trackContextMenu, setTrackContextMenu] =
    useState<TrackContextMenuState | null>(null);
  const [tagInspectorTrack, setTagInspectorTrack] = useState<ScannedTrack | null>(null);
  const [tagInspectorTags, setTagInspectorTags] = useState<TrackRawTags>({});
  const [tagInspectorError, setTagInspectorError] = useState<string | null>(null);
  const [isTagInspectorOpen, setIsTagInspectorOpen] = useState(false);
  const [isTagInspectorLoading, setIsTagInspectorLoading] = useState(false);
  const [editTagTracks, setEditTagTracks] = useState<ScannedTrack[]>([]);
  const [editTagError, setEditTagError] = useState<string | null>(null);
  const [isEditTagsDialogOpen, setIsEditTagsDialogOpen] = useState(false);
  const [isEditingTags, setIsEditingTags] = useState(false);
  const [exportTracks, setExportTracks] = useState<ScannedTrack[]>([]);
  const [exportError, setExportError] = useState<string | null>(null);
  const [isExportDialogOpen, setIsExportDialogOpen] = useState(false);
  const [isExportingField, setIsExportingField] = useState(false);
  const [isRandomPlayPending, setIsRandomPlayPending] = useState(false);
  const [hasHydrated, setHasHydrated] = useState(false);
  const [trackTableScrollTop, setTrackTableScrollTop] = useState(0);
  const [trackTableViewportHeight, setTrackTableViewportHeight] = useState(0);
  const resizeStateRef = useRef<ResizeState | null>(null);
  const visibleDropIndicatorRef = useRef<DropIndicator | null>(null);
  const persistedTrackTable = useMemo(
    () => normalizeTrackTableSettings(settings, columns),
    [columns, settings],
  );
  const expandedAlbumIdSet = useMemo(
    () => new Set(expandedAlbumIds),
    [expandedAlbumIds],
  );
  const selectedTrackSet = useMemo(
    () => new Set(selectedTrackIds),
    [selectedTrackIds],
  );

  useEffect(() => {
    setVisibleColumns(persistedTrackTable.visibleColumns);
    setColumnWidths(persistedTrackTable.columnWidths);
    setSortCriteria(trackTableToCriteria(persistedTrackTable));
    setHasHydrated(true);
  }, [persistedTrackTable]);

  useEffect(() => {
    if (!resizingColumn) {
      return;
    }

    const handlePointerMove = (event: PointerEvent) => {
      const resizeState = resizeStateRef.current;
      if (!resizeState) {
        return;
      }

      const nextWidths = resizeAdjacentColumns(
        resizeState.key,
        resizeState.partnerKey,
        resizeState.startWidth,
        resizeState.startPartnerWidth,
        event.clientX - resizeState.startX,
      );

      setColumnWidths((current) => ({
        ...current,
        ...nextWidths,
      }));
    };

    const handlePointerUp = () => {
      resizeStateRef.current = null;
      setResizingColumn(null);
    };

    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';

    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [resizingColumn]);

  useEffect(() => {
    if (!draggedVisibleColumn) {
      return;
    }

    const handlePointerMove = (event: PointerEvent) => {
      const element = document
        .elementFromPoint(event.clientX, event.clientY)
        ?.closest<HTMLElement>('[data-reorder-key]');

      if (!element) {
        visibleDropIndicatorRef.current = null;
        setVisibleDropIndicator(null);
        return;
      }

      const key = element.dataset.reorderKey;
      if (!key || key === draggedVisibleColumn) {
        visibleDropIndicatorRef.current = null;
        setVisibleDropIndicator(null);
        return;
      }

      const bounds = element.getBoundingClientRect();
      const nextIndicator: DropIndicator = {
        key,
        position:
          event.clientY - bounds.top < bounds.height / 2 ? 'before' : 'after',
      };

      visibleDropIndicatorRef.current = nextIndicator;
      setVisibleDropIndicator(nextIndicator);
    };

    const handlePointerUp = () => {
      const nextIndicator = visibleDropIndicatorRef.current;

      if (nextIndicator) {
        setVisibleColumns((current) =>
          moveListItem(
            current,
            draggedVisibleColumn,
            nextIndicator.key,
            nextIndicator.position,
          ),
        );
      }

      visibleDropIndicatorRef.current = null;
      setDraggedVisibleColumn(null);
      setVisibleDropIndicator(null);
    };

    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    document.body.style.cursor = 'grabbing';
    document.body.style.userSelect = 'none';

    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
  }, [draggedVisibleColumn]);

  useEffect(() => {
    if (!hasHydrated) {
      return;
    }

    const nextTrackTable = normalizeTrackTableSettings(
      buildTrackTableSettings(visibleColumns, columnWidths, sortCriteria),
      columns,
    );

    if (trackTableSettingsEqual(nextTrackTable, persistedTrackTable)) {
      return;
    }

    const timeout = window.setTimeout(() => {
      onTrackTableChange(nextTrackTable);
    }, 240);

    return () => window.clearTimeout(timeout);
  }, [
    columnWidths,
    columns,
    hasHydrated,
    onTrackTableChange,
    persistedTrackTable,
    sortCriteria,
    visibleColumns,
  ]);

  useEffect(() => {
    if (
      !isLayoutDialogOpen &&
      !isTagInspectorOpen &&
      !isEditTagsDialogOpen &&
      !isExportDialogOpen
    ) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsLayoutDialogOpen(false);
        setIsTagInspectorOpen(false);
        setIsEditTagsDialogOpen(false);
        setIsExportDialogOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isEditTagsDialogOpen, isExportDialogOpen, isLayoutDialogOpen, isTagInspectorOpen]);

  useEffect(() => {
    if (!albumContextMenu && !trackContextMenu) {
      return undefined;
    }

    const closeMenu = (event?: Event) => {
      if (event?.target instanceof Node && menuRef.current?.contains(event.target)) {
        return;
      }

      if (event instanceof KeyboardEvent && event.key !== 'Escape') {
        return;
      }

      setAlbumContextMenu(null);
      setTrackContextMenu(null);
    };

    window.addEventListener('pointerdown', closeMenu);
    window.addEventListener('resize', closeMenu);
    window.addEventListener('keydown', closeMenu);

    return () => {
      window.removeEventListener('pointerdown', closeMenu);
      window.removeEventListener('resize', closeMenu);
      window.removeEventListener('keydown', closeMenu);
    };
  }, [albumContextMenu, trackContextMenu]);

  const sortSummary = useMemo(
    () => describeSortCriteria(sortCriteria, columnLookup),
    [columnLookup, sortCriteria],
  );

  const trackSearchTextLookup = useMemo(
    () => new Map(tracks.map((track) => [track.id, buildTrackSearchText(track)])),
    [tracks],
  );

  const sortedTracks = useMemo(
    () => [...tracks].sort((left, right) => compareTracks(left, right, sortCriteria)),
    [sortCriteria, tracks],
  );

  const filteredTracks = useMemo(() => {
    const query = deferredFilter.trim().toLowerCase();
    if (!query) {
      return sortedTracks;
    }

    return sortedTracks.filter((track) =>
      (trackSearchTextLookup.get(track.id) ?? '').includes(query),
    );
  }, [deferredFilter, sortedTracks, trackSearchTextLookup]);
  const filteredTrackLookup = useMemo(
    () => new Map(filteredTracks.map((track) => [track.id, track])),
    [filteredTracks],
  );
  const albumCards = useMemo(
    () => new Map(buildAlbumCards(filteredTracks).map((album) => [album.id, album])),
    [filteredTracks],
  );
  const albumGroups = useMemo(
    () => buildAlbumGroups(filteredTracks, albumCards),
    [albumCards, filteredTracks],
  );
  const albumGroupLookup = useMemo(
    () => new Map(albumGroups.map((group) => [group.albumId, group])),
    [albumGroups],
  );
  const albumGroupIds = useMemo(
    () => albumGroups.map((group) => group.albumId),
    [albumGroups],
  );
  const visibleTracksInOrder = useMemo(
    () =>
      albumGroups.flatMap((group) =>
        expandedAlbumIdSet.has(group.albumId) ? group.tracks : [],
      ),
    [albumGroups, expandedAlbumIdSet],
  );
  const visibleTrackIdsInOrder = useMemo(
    () => visibleTracksInOrder.map((track) => track.id),
    [visibleTracksInOrder],
  );
  const virtualizedTrackTable = useMemo(
    () => buildVirtualizedTrackTable(albumGroups, expandedAlbumIdSet),
    [albumGroups, expandedAlbumIdSet],
  );
  const { bottomSpacerHeight, topSpacerHeight, visibleRows } = useMemo(() => {
    if (virtualizedTrackTable.rows.length === 0) {
      return {
        bottomSpacerHeight: 0,
        topSpacerHeight: 0,
        visibleRows: [] as TrackTableRow[],
      };
    }

    const viewportHeight =
      trackTableViewportHeight > 0
        ? trackTableViewportHeight
        : trackTableFallbackViewportHeight;
    const startOffset = Math.max(trackTableScrollTop - trackTableOverscanPx, 0);
    const endOffset = Math.min(
      virtualizedTrackTable.totalHeight,
      trackTableScrollTop + viewportHeight + trackTableOverscanPx,
    );
    const startIndex = findTrackTableRowIndex(
      virtualizedTrackTable.rowOffsets,
      startOffset,
    );
    const endIndex = Math.min(
      virtualizedTrackTable.rows.length,
      findTrackTableRowIndex(
        virtualizedTrackTable.rowOffsets,
        Math.max(endOffset - 1, 0),
      ) + 1,
    );

    return {
      bottomSpacerHeight:
        virtualizedTrackTable.totalHeight -
        (virtualizedTrackTable.rowOffsets[endIndex] ?? virtualizedTrackTable.totalHeight),
      topSpacerHeight: virtualizedTrackTable.rowOffsets[startIndex] ?? 0,
      visibleRows: virtualizedTrackTable.rows.slice(startIndex, endIndex),
    };
  }, [trackTableScrollTop, trackTableViewportHeight, virtualizedTrackTable]);

  useEffect(() => {
    const availableAlbumIds = new Set(albumGroupIds);

    setExpandedAlbumIds((current) => {
      const next = current.filter((albumId) => availableAlbumIds.has(albumId));
      return stringListsEqual(current, next) ? current : next;
    });
  }, [albumGroupIds]);

  useEffect(() => {
    const availableIds = new Set(visibleTrackIdsInOrder);

    setSelectedTrackIds((current) => {
      const next = current.filter((trackId) => availableIds.has(trackId));
      return stringListsEqual(current, next) ? current : next;
    });

    setSelectionAnchorId((current) =>
      current && availableIds.has(current) ? current : null,
    );
  }, [visibleTrackIdsInOrder]);

  useEffect(() => {
    if (!isActive || filteredTracks.length === 0) {
      return;
    }

    const element = trackTableScrollRef.current;
    if (!element) {
      return;
    }

    const measure = () => {
      setTrackTableViewportHeight(element.clientHeight);
      setTrackTableScrollTop(element.scrollTop);
    };

    measure();

    const resizeObserver =
      typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(() => measure())
        : null;

    resizeObserver?.observe(element);
    window.addEventListener('resize', measure);

    return () => {
      resizeObserver?.disconnect();
      window.removeEventListener('resize', measure);
    };
  }, [filteredTracks.length, isActive]);

  useEffect(() => {
    if (!isActive) {
      return;
    }

    const element = trackTableScrollRef.current;
    if (!element) {
      return;
    }

    const maxScrollTop = Math.max(
      virtualizedTrackTable.totalHeight - trackTableViewportHeight,
      0,
    );

    if (element.scrollTop > maxScrollTop) {
      element.scrollTop = maxScrollTop;
      setTrackTableScrollTop(maxScrollTop);
    }
  }, [isActive, trackTableViewportHeight, virtualizedTrackTable.totalHeight]);

  useEffect(() => {
    const element = trackTableScrollRef.current;
    if (!element) {
      return;
    }

    element.scrollTop = 0;
    setTrackTableScrollTop(0);
  }, [deferredFilter]);

  function toggleColumn(key: string) {
    setVisibleColumns((current) => {
      if (current.includes(key)) {
        return current.length > 1
          ? current.filter((columnKey) => columnKey !== key)
          : current;
      }

      return [...current, key];
    });
  }

  function handleVisibleColumnDragStart(
    event: ReactPointerEvent<HTMLElement>,
    key: string,
  ) {
    event.preventDefault();
    event.stopPropagation();
    visibleDropIndicatorRef.current = null;
    setDraggedVisibleColumn(key);
    setVisibleDropIndicator(null);
  }

  function handleResizeStart(
    event: ReactPointerEvent<HTMLDivElement>,
    key: string,
    partnerKey: string | null,
  ) {
    event.preventDefault();
    event.stopPropagation();

    if (!partnerKey) {
      return;
    }

    resizeStateRef.current = {
      key,
      partnerKey,
      startX: event.clientX,
      startWidth: columnWidths[key] ?? getDefaultColumnWidth(key),
      startPartnerWidth:
        columnWidths[partnerKey] ?? getDefaultColumnWidth(partnerKey),
    };
    setResizingColumn(key);
  }

  function handleTrackClick(
    event: ReactMouseEvent<HTMLTableRowElement>,
    trackId: string,
  ) {
    const orderedIds = visibleTrackIdsInOrder;

    if (event.shiftKey && selectionAnchorId) {
      const anchorIndex = orderedIds.indexOf(selectionAnchorId);
      const targetIndex = orderedIds.indexOf(trackId);

      if (anchorIndex !== -1 && targetIndex !== -1) {
        const [start, end] =
          anchorIndex < targetIndex
            ? [anchorIndex, targetIndex]
            : [targetIndex, anchorIndex];
        setSelectedTrackIds(orderedIds.slice(start, end + 1));
        return;
      }
    }

    if (event.ctrlKey || event.metaKey) {
      setSelectedTrackIds((current) =>
        current.includes(trackId)
          ? current.filter((id) => id !== trackId)
          : [...current, trackId],
      );
      setSelectionAnchorId(trackId);
      return;
    }

    setSelectedTrackIds((current) =>
      current.length > 1 && current.includes(trackId) ? current : [trackId],
    );
    setSelectionAnchorId(trackId);
  }

  function toggleAlbumExpansion(albumId: string) {
    setExpandedAlbumIds((current) =>
      current.includes(albumId)
        ? current.filter((currentAlbumId) => currentAlbumId !== albumId)
        : [...current, albumId],
    );
  }

  function expandVisibleAlbums() {
    setExpandedAlbumIds(albumGroupIds);
  }

  function collapseAllAlbums() {
    setExpandedAlbumIds([]);
  }

  function handleAlbumToggle(
    event: ReactMouseEvent<HTMLButtonElement>,
    albumId: string,
  ) {
    event.preventDefault();
    event.stopPropagation();
    toggleAlbumExpansion(albumId);
  }

  function handleTrackDoubleClick(track: ScannedTrack) {
    const selectedTracks = selectedTrackSet.has(track.id)
      ? selectTracksInOrder(visibleTracksInOrder, selectedTrackIds)
      : [track];

    void onPlayTracks(selectedTracks);
  }

  function handleTrackContextMenu(
    event: ReactMouseEvent<HTMLTableRowElement>,
    track: ScannedTrack,
  ) {
    event.preventDefault();
    event.stopPropagation();

    const nextSelectedTrackIds = selectedTrackSet.has(track.id)
      ? selectTrackIdsInOrder(visibleTracksInOrder, selectedTrackIds)
      : [track.id];

    if (!selectedTrackSet.has(track.id)) {
      setSelectedTrackIds(nextSelectedTrackIds);
      setSelectionAnchorId(track.id);
    }

    const placement = resolveContextMenuPosition(
      trackTableScrollRef.current,
      event.clientX,
      event.clientY,
      240,
    );

    setAlbumContextMenu(null);
    setTrackContextMenu({
      x: placement.x,
      y: placement.y,
      trackIds: nextSelectedTrackIds,
      primaryTrackId: track.id,
    });
  }

  function handleAlbumContextMenu(
    event: ReactMouseEvent<HTMLTableRowElement>,
    albumId: string,
  ) {
    event.preventDefault();
    event.stopPropagation();
    const placement = resolveContextMenuPosition(
      trackTableScrollRef.current,
      event.clientX,
      event.clientY,
      240,
    );

    setTrackContextMenu(null);
    setAlbumContextMenu({
      albumId,
      x: placement.x,
      y: placement.y,
    });
  }

  async function runTrackContextAction(
    action: (tracks: ScannedTrack[]) => void | Promise<void>,
  ) {
    if (!trackContextMenu) {
      return;
    }

    const selectedTracks = selectTracksInOrder(
      visibleTracksInOrder,
      trackContextMenu.trackIds,
    );
    if (selectedTracks.length === 0) {
      setTrackContextMenu(null);
      return;
    }

    setTrackContextMenu(null);
    await action(selectedTracks);
  }

  async function runAlbumContextAction(
    action: (albumId: string) => void | Promise<void>,
  ) {
    if (!albumContextMenu) {
      return;
    }

    const { albumId } = albumContextMenu;
    setAlbumContextMenu(null);
    await action(albumId);
  }

  async function runShowInExplorer() {
    if (!trackContextMenu || trackContextMenu.trackIds.length !== 1) {
      return;
    }

    const track = filteredTrackLookup.get(trackContextMenu.primaryTrackId) ?? null;

    if (!track) {
      setTrackContextMenu(null);
      return;
    }

    setTrackContextMenu(null);
    await onShowInExplorer(track);
  }

  async function runShowAllTags() {
    if (!trackContextMenu || trackContextMenu.trackIds.length !== 1) {
      return;
    }

    const track = filteredTrackLookup.get(trackContextMenu.primaryTrackId) ?? null;

    if (!track) {
      setTrackContextMenu(null);
      return;
    }

    setTrackContextMenu(null);
    setTagInspectorTrack(track);
    setTagInspectorTags({});
    setTagInspectorError(null);
    setIsTagInspectorOpen(true);
    setIsTagInspectorLoading(true);

    try {
      const tags = await readTrackRawTags(track.path, track.rawTags);
      setTagInspectorTags(tags);
    } catch (reason) {
      setTagInspectorError(String(reason));
    } finally {
      setIsTagInspectorLoading(false);
    }
  }

  function openExportDialog(tracksToExport: ScannedTrack[]) {
    if (tracksToExport.length === 0) {
      return;
    }

    setExportTracks(tracksToExport);
    setExportError(null);
    setIsExportDialogOpen(true);
  }

  function openEditTagsDialog(tracksToEdit: ScannedTrack[]) {
    if (tracksToEdit.length === 0) {
      return;
    }

    setEditTagTracks(tracksToEdit);
    setEditTagError(null);
    setIsEditTagsDialogOpen(true);
  }

  function runAlbumExportField() {
    if (!albumContextMenu) {
      return;
    }

    const group = albumGroupLookup.get(albumContextMenu.albumId) ?? null;
    setAlbumContextMenu(null);
    openExportDialog(group?.tracks ?? []);
  }

  function runAlbumEditTags() {
    if (!albumContextMenu) {
      return;
    }

    const group = albumGroupLookup.get(albumContextMenu.albumId) ?? null;
    setAlbumContextMenu(null);
    openEditTagsDialog(group?.tracks ?? []);
  }

  function runTrackExportField() {
    if (!trackContextMenu) {
      return;
    }

    const selectedTracks = selectTracksInOrder(
      visibleTracksInOrder,
      trackContextMenu.trackIds,
    );
    setTrackContextMenu(null);
    openExportDialog(selectedTracks);
  }

  function runTrackEditTags() {
    if (!trackContextMenu) {
      return;
    }

    const selectedTracks = selectTracksInOrder(
      visibleTracksInOrder,
      trackContextMenu.trackIds,
    );
    setTrackContextMenu(null);
    openEditTagsDialog(selectedTracks);
  }

  async function handleExportField(fieldKey: string, tagName: string) {
    setExportError(null);
    setIsExportingField(true);

    try {
      await onExportField(exportTracks, fieldKey, tagName);
      setIsExportDialogOpen(false);
    } catch (reason) {
      setExportError(String(reason));
    } finally {
      setIsExportingField(false);
    }
  }

  async function handleEditTrackTags(updates: TrackTagEditUpdate[]) {
    setEditTagError(null);
    setIsEditingTags(true);

    try {
      await onEditTrackTags(editTagTracks, updates);
      setIsEditTagsDialogOpen(false);
    } catch (reason) {
      setEditTagError(String(reason));
    } finally {
      setIsEditingTags(false);
    }
  }

  function addSortCriterion() {
    const nextColumn = columns.find(
      (column) => !sortCriteria.some((criterion) => criterion.key === column.key),
    );

    if (!nextColumn) {
      return;
    }

    setSortCriteria((current) => [
      ...current,
      { key: nextColumn.key, direction: 'asc' },
    ]);
  }

  function updateSortCriterionKey(index: number, nextKey: string) {
    setSortCriteria((current) =>
      current.map((criterion, currentIndex) =>
        currentIndex === index ? { ...criterion, key: nextKey } : criterion,
      ),
    );
  }

  function updateSortCriterionDirection(
    index: number,
    direction: TrackSortDirection,
  ) {
    setSortCriteria((current) =>
      current.map((criterion, currentIndex) =>
        currentIndex === index ? { ...criterion, direction } : criterion,
      ),
    );
  }

  function moveSortCriterion(index: number, offset: number) {
    setSortCriteria((current) => {
      const targetIndex = index + offset;
      if (targetIndex < 0 || targetIndex >= current.length) {
        return current;
      }

      const next = [...current];
      const [criterion] = next.splice(index, 1);
      next.splice(targetIndex, 0, criterion);
      return next;
    });
  }

  function removeSortCriterion(index: number) {
    setSortCriteria((current) =>
      current.length > 1
        ? current.filter((_, currentIndex) => currentIndex !== index)
        : current,
    );
  }

  function resetSortToClassicalDefault() {
    setSortCriteria(buildDefaultSortCriteria(columns.map((column) => column.key)));
  }

  async function handleRandomPlay() {
    if (filteredTracks.length === 0 || isRandomPlayPending) {
      return;
    }

    setIsRandomPlayPending(true);
    try {
      await onReplaceQueueAndPlayTracks(pickRandomTracks(filteredTracks, 100));
    } finally {
      setIsRandomPlayPending(false);
    }
  }

  return (
    <div className="pane-stack track-pane" hidden={!isActive}>
      <SectionCard hideHeader>
        <div className="track-pane__layout">
          <div className="track-controls">
            <label className="field-label">
              Filter
              <ClearableInput
                onClear={() => setFilter('')}
                placeholder="Filter by title, album, composer, conductor, path"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
              />
            </label>
            <div className="track-controls__meta">
              <div className="track-controls__stats">
                <span className="pane-chip">{filteredTracks.length} tracks</span>
                <span className="pane-chip">{albumGroups.length} albums</span>
                <span className="pane-chip">{selectedTrackIds.length} selected</span>
                <span className="pane-chip">{expandedAlbumIds.length} expanded</span>
              </div>
              <div className="track-controls__actions">
                <button
                  className="ghost-button"
                  disabled={filteredTracks.length === 0 || isRandomPlayPending}
                  onClick={() => void handleRandomPlay()}
                  type="button"
                >
                  {isRandomPlayPending ? 'Starting…' : 'Random play'}
                </button>
                <button
                  className="ghost-button"
                  disabled={albumGroups.length === 0 || expandedAlbumIds.length === albumGroups.length}
                  onClick={expandVisibleAlbums}
                  type="button"
                >
                  Expand visible
                </button>
                <button
                  className="ghost-button"
                  disabled={expandedAlbumIds.length === 0}
                  onClick={collapseAllAlbums}
                  type="button"
                >
                  Collapse all
                </button>
                <button
                  className="ghost-button"
                  onClick={() => setIsLayoutDialogOpen(true)}
                  type="button"
                >
                  Layout & Sort
                </button>
              </div>
            </div>
          </div>

          {filteredTracks.length === 0 ? (
            <div className="placeholder-pane">
              <strong>No tracks match the current filters</strong>
              <p>Adjust the filter text to bring albums back into view.</p>
            </div>
          ) : (
            <div
              className="track-table-shell track-table-shell--clamped track-table-shell--virtualized"
              onScroll={(event) => setTrackTableScrollTop(event.currentTarget.scrollTop)}
              ref={trackTableScrollRef}
            >
              <table className="track-table track-table--grouped">
                <colgroup>
                  {visibleColumns.map((key) => (
                    <col
                      key={key}
                      style={{
                        width: `${buildColumnWeight(key, columnWidths, visibleColumns)}%`,
                      }}
                    />
                  ))}
                </colgroup>
                <thead>
                  <tr>
                    {visibleColumns.map((key, index) => {
                      const column = columnLookup.get(key);
                      const partnerKey = visibleColumns[index + 1] ?? null;

                      return (
                        <th className="track-table__header" key={key}>
                          <div className="track-table__header-inner">
                            <span title={column?.label ?? key}>{column?.label ?? key}</span>
                            {partnerKey ? (
                              <div
                                className="track-table__resize-handle"
                                onPointerDown={(event) =>
                                  handleResizeStart(event, key, partnerKey)
                                }
                                role="presentation"
                              />
                            ) : null}
                          </div>
                        </th>
                      );
                    })}
                  </tr>
                </thead>
                <tbody>
                  {topSpacerHeight > 0 ? (
                    <tr aria-hidden className="track-table__spacer-row">
                      <td
                        colSpan={Math.max(visibleColumns.length, 1)}
                        style={{ height: `${topSpacerHeight}px` }}
                      />
                    </tr>
                  ) : null}

                  {visibleRows.map((row) => {
                    if (row.kind === 'album') {
                      const albumTitle =
                        row.group.album?.title ||
                        row.group.tracks[0]?.mappedFields.album?.[0] ||
                        'Unknown album';
                      const isExpanded = expandedAlbumIdSet.has(row.group.albumId);

                      return (
                        <tr
                          className={[
                            'track-table__album-row',
                            isExpanded ? 'track-table__album-row--expanded' : '',
                          ]
                            .filter(Boolean)
                            .join(' ')}
                          key={row.key}
                          onClick={() => onOpenAlbum(row.group.albumId)}
                          onContextMenu={(event) =>
                            handleAlbumContextMenu(event, row.group.albumId)
                          }
                          title="Click to open the album pane. Use the arrow to expand tracks."
                        >
                          <td colSpan={Math.max(visibleColumns.length, 1)}>
                            <div className="track-table__album-meta">
                              <button
                                aria-expanded={isExpanded}
                                aria-label={`${isExpanded ? 'Collapse' : 'Expand'} ${albumTitle}`}
                                className="track-table__album-toggle"
                                onClick={(event) => handleAlbumToggle(event, row.group.albumId)}
                                type="button"
                              >
                                {isExpanded ? '▾' : '▸'}
                              </button>
                              <HoverScrollText
                                className="track-table__album-title"
                                speed={44}
                                text={albumTitle}
                              />
                              <span className="track-table__album-count">
                                {row.group.tracks.length}{' '}
                                {row.group.tracks.length === 1 ? 'track' : 'tracks'}
                              </span>
                            </div>
                          </td>
                        </tr>
                      );
                    }

                    const { track } = row;

                    return (
                      <tr
                        aria-selected={selectedTrackSet.has(track.id)}
                        className={[
                          'track-table__row--playable',
                          selectedTrackSet.has(track.id) ? 'track-table__row--selected' : '',
                        ]
                          .filter(Boolean)
                          .join(' ')}
                        key={row.key}
                        onClick={(event) => handleTrackClick(event, track.id)}
                        onContextMenu={(event) => handleTrackContextMenu(event, track)}
                        onDoubleClick={() => handleTrackDoubleClick(track)}
                      >
                        {visibleColumns.map((key) => (
                          <td
                            className={key === 'path' ? 'track-path' : undefined}
                            key={`${track.id}-${key}`}
                          >
                            <HoverScrollText
                              className="track-table__cell-text"
                              speed={36}
                              text={getTrackColumnValue(track, key) || '-'}
                            />
                          </td>
                        ))}
                      </tr>
                    );
                  })}

                  {bottomSpacerHeight > 0 ? (
                    <tr aria-hidden className="track-table__spacer-row">
                      <td
                        colSpan={Math.max(visibleColumns.length, 1)}
                        style={{ height: `${bottomSpacerHeight}px` }}
                      />
                    </tr>
                  ) : null}
                </tbody>
              </table>

              {albumContextMenu ? (
                <div
                  className="album-context-menu album-context-menu--pane-bound"
                  ref={menuRef}
                  style={{ left: `${albumContextMenu.x}px`, top: `${albumContextMenu.y}px` }}
                >
                  <button
                    onClick={() => void runAlbumContextAction(onAddAlbumToPlaylist)}
                    type="button"
                  >
                    Add to playlist
                  </button>
                  <button onClick={() => void runAlbumContextAction(onAddAlbumToQueue)} type="button">
                    Add to queue
                  </button>
                  <button onClick={() => void runAlbumContextAction(onReplaceQueue)} type="button">
                    Put in queue
                  </button>
                  <button onClick={() => void runAlbumContextAction(onPlayAlbum)} type="button">
                    Play album
                  </button>
                  <button onClick={() => runAlbumEditTags()} type="button">
                    Edit tags
                  </button>
                  <button onClick={() => runAlbumExportField()} type="button">
                    Export field
                  </button>
                  <button onClick={() => void runAlbumContextAction(onGoToDirectory)} type="button">
                    Go to directory
                  </button>
                </div>
              ) : null}

              {trackContextMenu ? (
                <div
                  className="album-context-menu album-context-menu--pane-bound"
                  ref={menuRef}
                  style={{ left: `${trackContextMenu.x}px`, top: `${trackContextMenu.y}px` }}
                >
                  <button
                    onClick={() => void runTrackContextAction(onAddToPlaylist)}
                    type="button"
                  >
                    Add to playlist
                  </button>
                  <button onClick={() => void runTrackContextAction(onAddToQueue)} type="button">
                    Add to queue
                  </button>
                  <button onClick={() => void runTrackContextAction(onPlayTracks)} type="button">
                    Play
                  </button>
                  <button onClick={() => runTrackEditTags()} type="button">
                    Edit tags
                  </button>
                  <button onClick={() => runTrackExportField()} type="button">
                    Export field
                  </button>
                  {trackContextMenu.trackIds.length === 1 ? (
                    <button onClick={() => void runShowAllTags()} type="button">
                      Show all tags
                    </button>
                  ) : null}
                  {trackContextMenu.trackIds.length === 1 ? (
                    <button onClick={() => void runShowInExplorer()} type="button">
                      Show in Explorer
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>
          )}
        </div>
      </SectionCard>

      {isLayoutDialogOpen ? (
        <div
          className="dialog-backdrop"
          onClick={() => setIsLayoutDialogOpen(false)}
          role="presentation"
        >
          <div
            className="dialog-card"
            onClick={(event) => event.stopPropagation()}
            role="dialog"
            aria-modal="true"
            aria-labelledby="track-layout-dialog-title"
          >
            <div className="dialog-card__header">
              <div>
                <p className="section-card__eyebrow">Tracks</p>
                <h3 id="track-layout-dialog-title">Layout and sort</h3>
              </div>
              <button
                className="ghost-button"
                onClick={() => setIsLayoutDialogOpen(false)}
                type="button"
              >
                Close
              </button>
            </div>

            <p className="dialog-card__copy">
              Pick which row columns are visible, drag the visible-column list to
              reorder it, and build a multi-level sort priority for the grouped
              track view. Album is now rendered as a merged row inside the table.
            </p>

            <div className="dialog-sections">
              <section className="dialog-section">
                <div className="dialog-section__header">
                  <h4>Visible columns</h4>
                </div>
                <p className="dialog-section__note">
                  Drag the handle on each row up or down to change the table order.
                </p>
                <ul className="reorder-list">
                  {visibleColumns.map((key) => {
                    const column = columnLookup.get(key);
                    const isDropBefore =
                      visibleDropIndicator?.key === key &&
                      visibleDropIndicator.position === 'before';
                    const isDropAfter =
                      visibleDropIndicator?.key === key &&
                      visibleDropIndicator.position === 'after';

                    return (
                      <li
                        className={[
                          'reorder-list__item',
                          draggedVisibleColumn === key
                            ? 'reorder-list__item--dragging'
                            : '',
                          isDropBefore ? 'reorder-list__item--drop-before' : '',
                          isDropAfter ? 'reorder-list__item--drop-after' : '',
                        ]
                          .filter(Boolean)
                          .join(' ')}
                        data-reorder-key={key}
                        key={key}
                      >
                        <div className="reorder-list__row">
                          <span
                            className="reorder-list__handle"
                            onPointerDown={(event) =>
                              handleVisibleColumnDragStart(event, key)
                            }
                            role="presentation"
                          >
                            Drag
                          </span>
                          <strong>{column?.label ?? key}</strong>
                          <span className="reorder-list__meta">
                            {columnWidths[key] ?? getDefaultColumnWidth(key)} px
                          </span>
                          <button
                            className="ghost-button"
                            disabled={visibleColumns.length === 1}
                            onClick={() => toggleColumn(key)}
                            type="button"
                          >
                            Hide
                          </button>
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </section>

              <section className="dialog-section">
                <div className="dialog-section__header">
                  <h4>Available columns</h4>
                </div>
                <div className="column-picker column-picker--dialog">
                  {columns.map((column) => (
                    <label className="column-toggle" key={column.key}>
                      <input
                        checked={visibleColumns.includes(column.key)}
                        onChange={() => toggleColumn(column.key)}
                        type="checkbox"
                      />
                      <span>{column.label}</span>
                    </label>
                  ))}
                </div>
              </section>

              <section className="dialog-section">
                <div className="dialog-section__header">
                  <h4>Sort priority</h4>
                  <div className="inline-actions">
                    <button
                      className="ghost-button"
                      onClick={resetSortToClassicalDefault}
                      type="button"
                    >
                      Classical default
                    </button>
                    <button
                      className="ghost-button"
                      disabled={sortCriteria.length >= columns.length}
                      onClick={addSortCriterion}
                      type="button"
                    >
                      Add level
                    </button>
                  </div>
                </div>
                <p className="dialog-section__note">
                  Album groups follow the configured sort priority. Tracks inside
                  each album keep album order.
                </p>
                <div className="sort-criteria">
                  {sortCriteria.map((criterion, index) => (
                    <div className="sort-criteria__row" key={`${criterion.key}-${index}`}>
                      <span className="sort-criteria__index">{index + 1}</span>

                      <label className="field-label">
                        Column
                        <select
                          value={criterion.key}
                          onChange={(event) =>
                            updateSortCriterionKey(index, event.target.value)
                          }
                        >
                          {columns
                            .filter(
                              (column) =>
                                column.key === criterion.key ||
                                !sortCriteria.some(
                                  (item, itemIndex) =>
                                    itemIndex !== index && item.key === column.key,
                                ),
                            )
                            .map((column) => (
                              <option key={column.key} value={column.key}>
                                {column.label}
                              </option>
                            ))}
                        </select>
                      </label>

                      <label className="field-label">
                        Direction
                        <select
                          value={criterion.direction}
                          onChange={(event) =>
                            updateSortCriterionDirection(
                              index,
                              event.target.value as TrackSortDirection,
                            )
                          }
                        >
                          <option value="asc">Ascending</option>
                          <option value="desc">Descending</option>
                        </select>
                      </label>

                      <div className="sort-criteria__actions">
                        <button
                          className="ghost-button"
                          disabled={index === 0}
                          onClick={() => moveSortCriterion(index, -1)}
                          type="button"
                        >
                          Up
                        </button>
                        <button
                          className="ghost-button"
                          disabled={index === sortCriteria.length - 1}
                          onClick={() => moveSortCriterion(index, 1)}
                          type="button"
                        >
                          Down
                        </button>
                        <button
                          className="ghost-button"
                          disabled={sortCriteria.length === 1}
                          onClick={() => removeSortCriterion(index)}
                          type="button"
                        >
                          Remove
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </section>
            </div>
          </div>
        </div>
      ) : null}

      <TrackRawTagsDialog
        error={tagInspectorError}
        isLoading={isTagInspectorLoading}
        isOpen={isTagInspectorOpen}
        onClose={() => setIsTagInspectorOpen(false)}
        tags={tagInspectorTags}
        track={tagInspectorTrack}
      />

      <EditTrackTagsDialog
        error={editTagError}
        isOpen={isEditTagsDialogOpen}
        isSubmitting={isEditingTags}
        onClose={() => setIsEditTagsDialogOpen(false)}
        onSubmit={handleEditTrackTags}
        tracks={editTagTracks}
      />

      <ExportFieldDialog
        error={exportError}
        fieldMappings={mappings}
        isOpen={isExportDialogOpen}
        isSubmitting={isExportingField}
        onAddTagOption={onRememberExportTag}
        onClose={() => setIsExportDialogOpen(false)}
        onSubmit={handleExportField}
        sessionTagOptions={sessionExportTags}
        tracks={exportTracks}
      />
    </div>
  );
}

function compareTracks(
  left: ScannedTrack,
  right: ScannedTrack,
  sortCriteria: TrackSortCriterion[],
): number {
  for (const criterion of sortCriteria) {
    const comparison = compareColumnValues(
      getTrackColumnValue(left, criterion.key),
      getTrackColumnValue(right, criterion.key),
    );

    if (comparison !== 0) {
      return criterion.direction === 'desc' ? comparison * -1 : comparison;
    }
  }

  return left.path.localeCompare(right.path, undefined, {
    numeric: true,
    sensitivity: 'base',
  });
}

function compareColumnValues(leftValue: string, rightValue: string): number {
  const leftNumber = parseSortableNumber(leftValue);
  const rightNumber = parseSortableNumber(rightValue);

  if (leftNumber !== null && rightNumber !== null && leftNumber !== rightNumber) {
    return leftNumber - rightNumber;
  }

  return leftValue.localeCompare(rightValue, undefined, {
    numeric: true,
    sensitivity: 'base',
  });
}

function parseSortableNumber(value: string): number | null {
  const matched = value.match(/\d+/);
  if (!matched) {
    return null;
  }

  const parsed = Number.parseInt(matched[0], 10);
  return Number.isNaN(parsed) ? null : parsed;
}

function buildTrackSearchText(track: ScannedTrack): string {
  return [track.fileName, track.path, ...Object.values(track.mappedFields).flat()]
    .join(' ')
    .toLowerCase();
}

function buildAlbumGroups(
  orderedTracks: ScannedTrack[],
  albumCards: Map<string, AlbumCardModel>,
): AlbumTrackGroup[] {
  const groups = new Map<string, ScannedTrack[]>();

  for (const track of orderedTracks) {
    const albumId = albumIdForTrack(track);
    const currentGroup = groups.get(albumId);

    if (currentGroup) {
      currentGroup.push(track);
      continue;
    }

    groups.set(albumId, [track]);
  }

  return Array.from(groups.entries()).map(([albumId, albumTracks]) => ({
    albumId,
    album: albumCards.get(albumId) ?? null,
    tracks: [...albumTracks].sort(compareTracksWithinAlbum),
  }));
}

function buildVirtualizedTrackTable(
  albumGroups: AlbumTrackGroup[],
  expandedAlbumIds: Set<string>,
): VirtualizedTrackTable {
  const rows: TrackTableRow[] = [];
  const rowOffsets = [0];

  for (const group of albumGroups) {
    rows.push({
      group,
      key: `album:${group.albumId}`,
      kind: 'album',
    });
    rowOffsets.push(rowOffsets[rowOffsets.length - 1] + trackTableAlbumRowHeight);

    if (!expandedAlbumIds.has(group.albumId)) {
      continue;
    }

    for (const track of group.tracks) {
      rows.push({
        albumId: group.albumId,
        key: `track:${track.id}`,
        kind: 'track',
        track,
      });
      rowOffsets.push(
        rowOffsets[rowOffsets.length - 1] + trackTableTrackRowHeight,
      );
    }
  }

  return {
    rowOffsets,
    rows,
    totalHeight: rowOffsets[rowOffsets.length - 1] ?? 0,
  };
}

function findTrackTableRowIndex(rowOffsets: number[], offset: number): number {
  const rowCount = Math.max(rowOffsets.length - 1, 0);

  if (rowCount === 0) {
    return 0;
  }

  let low = 0;
  let high = rowCount - 1;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);

    if (rowOffsets[mid] > offset) {
      high = mid - 1;
      continue;
    }

    if (rowOffsets[mid + 1] <= offset) {
      low = mid + 1;
      continue;
    }

    return mid;
  }

  return Math.max(0, Math.min(rowCount - 1, low));
}

function buildColumnWeight(
  key: string,
  columnWidths: Record<string, number>,
  visibleColumns: string[],
): number {
  const total = visibleColumns.reduce(
    (sum, columnKey) => sum + (columnWidths[columnKey] ?? getDefaultColumnWidth(columnKey)),
    0,
  );

  if (total <= 0) {
    return 100 / Math.max(visibleColumns.length, 1);
  }

  return ((columnWidths[key] ?? getDefaultColumnWidth(key)) / total) * 100;
}

function selectTrackIdsInOrder(
  orderedTracks: ScannedTrack[],
  selectedTrackIds: string[],
): string[] {
  const selectedTrackSet = new Set(selectedTrackIds);
  return orderedTracks
    .filter((track) => selectedTrackSet.has(track.id))
    .map((track) => track.id);
}

function selectTracksInOrder(
  orderedTracks: ScannedTrack[],
  selectedTrackIds: string[],
): ScannedTrack[] {
  const selectedTrackSet = new Set(selectedTrackIds);
  return orderedTracks.filter((track) => selectedTrackSet.has(track.id));
}

function moveListItem(
  current: string[],
  sourceKey: string,
  targetKey: string,
  position: DropPosition,
): string[] {
  const withoutSource = current.filter((key) => key !== sourceKey);
  const targetIndex = withoutSource.indexOf(targetKey);

  if (targetIndex === -1) {
    return current;
  }

  const insertIndex = position === 'after' ? targetIndex + 1 : targetIndex;
  withoutSource.splice(insertIndex, 0, sourceKey);
  return withoutSource;
}

function buildTrackTableSettings(
  visibleColumns: string[],
  columnWidths: Record<string, number>,
  sortCriteria: TrackSortCriterion[],
): TrackTableSettings {
  const normalizedCriteria =
    sortCriteria.length > 0
      ? sortCriteria
      : [{ key: defaultSortKeys[0], direction: 'asc' as TrackSortDirection }];
  const [primaryCriterion, ...secondaryCriteria] = normalizedCriteria;

  return {
    visibleColumns,
    columnWidths,
    sortKey: primaryCriterion.key,
    sortDirection: primaryCriterion.direction,
    secondarySort: secondaryCriteria,
  };
}

function trackTableToCriteria(
  settings: TrackTableSettings,
): TrackSortCriterion[] {
  return [
    { key: settings.sortKey, direction: settings.sortDirection },
    ...(settings.secondarySort ?? []),
  ];
}

function buildDefaultSortCriteria(availableKeys: string[]): TrackSortCriterion[] {
  const criteria = defaultSortKeys
    .filter((key) => availableKeys.includes(key))
    .map((key) => ({ key, direction: 'asc' as TrackSortDirection }));

  if (criteria.length > 0) {
    return criteria;
  }

  const fallbackKey = availableKeys[0] ?? 'title';
  return [{ key: fallbackKey, direction: 'asc' }];
}

function describeSortCriteria(
  criteria: TrackSortCriterion[],
  columnLookup: Map<string, { key: string; label: string }>,
): string {
  return criteria
    .map((criterion) => {
      const label = columnLookup.get(criterion.key)?.label ?? criterion.key;
      const direction = criterion.direction === 'desc' ? 'descending' : 'ascending';
      return `${label} (${direction})`;
    })
    .join(' -> ');
}

function getDefaultColumnWidth(key: string): number {
  switch (key) {
    case 'track_number':
    case 'disk_number':
      return 96;
    case 'year':
    case 'format':
    case 'duration':
      return 110;
    case 'file_name':
      return 220;
    case 'path':
      return 360;
    case 'title':
      return 280;
    case 'album':
      return 260;
    case 'composer':
    case 'conductor':
    case 'ensemble':
    case 'soloist':
      return 220;
    default:
      return 180;
  }
}

function clampColumnWidth(key: string, width: number): number {
  const { min, max } = getColumnWidthBounds(key);
  return Math.max(min, Math.min(max, Math.round(width)));
}

function getColumnWidthBounds(key: string): { min: number; max: number } {
  const min =
    key === 'path'
      ? 220
      : key === 'title' || key === 'album'
        ? 160
        : 96;

  const max =
    key === 'path'
      ? 1600
      : key === 'title'
        ? 1500
        : key === 'album'
          ? 1300
          : key === 'file_name'
            ? 1100
            : key === 'composer'
              ? 760
              : ['conductor', 'ensemble', 'soloist'].includes(key)
                ? 860
                : ['track_number', 'disk_number', 'year', 'format', 'duration'].includes(key)
                  ? 240
                  : 720;

  return {
    min,
    max,
  };
}

function resizeAdjacentColumns(
  key: string,
  partnerKey: string,
  startWidth: number,
  startPartnerWidth: number,
  delta: number,
): Record<string, number> {
  const primaryBounds = getColumnWidthBounds(key);
  const partnerBounds = getColumnWidthBounds(partnerKey);
  const minimumDelta = Math.max(
    primaryBounds.min - startWidth,
    startPartnerWidth - partnerBounds.max,
  );
  const maximumDelta = Math.min(
    primaryBounds.max - startWidth,
    startPartnerWidth - partnerBounds.min,
  );
  const boundedDelta =
    maximumDelta < minimumDelta
      ? 0
      : Math.max(minimumDelta, Math.min(maximumDelta, delta));

  return {
    [key]: clampColumnWidth(key, startWidth + boundedDelta),
    [partnerKey]: clampColumnWidth(
      partnerKey,
      startPartnerWidth - boundedDelta,
    ),
  };
}

function normalizeTrackTableSettings(
  settings: TrackTableSettings,
  columns: { key: string }[],
): TrackTableSettings {
  const availableKeys = columns.map((column) => column.key);
  const normalizedVisibleColumns = Array.from(
    new Set(
      settings.visibleColumns.filter((key) => availableKeys.includes(key)),
    ),
  );
  const nextWidths: Record<string, number> = {};

  for (const column of columns) {
    nextWidths[column.key] = clampColumnWidth(
      column.key,
      settings.columnWidths[column.key] ?? getDefaultColumnWidth(column.key),
    );
  }

  const visibleColumns =
    normalizedVisibleColumns.length > 0
      ? normalizedVisibleColumns
      : defaultTrackColumns.filter((key) => availableKeys.includes(key));
  const normalizedSortCriteria = normalizeSortCriteria(settings, availableKeys);
  const [primaryCriterion, ...secondaryCriteria] = normalizedSortCriteria;

  return {
    visibleColumns,
    columnWidths: nextWidths,
    sortKey: primaryCriterion.key,
    sortDirection: primaryCriterion.direction,
    secondarySort: secondaryCriteria,
  };
}

function normalizeSortCriteria(
  settings: TrackTableSettings,
  availableKeys: string[],
): TrackSortCriterion[] {
  const criteria = [
    { key: settings.sortKey, direction: settings.sortDirection },
    ...(settings.secondarySort ?? []),
  ];
  const normalized: TrackSortCriterion[] = [];
  const seen = new Set<string>();

  for (const criterion of criteria) {
    if (!availableKeys.includes(criterion.key) || seen.has(criterion.key)) {
      continue;
    }

    normalized.push({
      key: criterion.key,
      direction: criterion.direction ?? 'asc',
    });
    seen.add(criterion.key);
  }

  if (normalized.length > 0) {
    return normalized;
  }

  return buildDefaultSortCriteria(availableKeys);
}

function trackTableSettingsEqual(
  left: TrackTableSettings,
  right: TrackTableSettings,
): boolean {
  if (left.sortKey !== right.sortKey || left.sortDirection !== right.sortDirection) {
    return false;
  }

  if (left.secondarySort.length !== right.secondarySort.length) {
    return false;
  }

  for (let index = 0; index < left.secondarySort.length; index += 1) {
    if (
      left.secondarySort[index].key !== right.secondarySort[index].key ||
      left.secondarySort[index].direction !==
        right.secondarySort[index].direction
    ) {
      return false;
    }
  }

  if (left.visibleColumns.length !== right.visibleColumns.length) {
    return false;
  }

  for (let index = 0; index < left.visibleColumns.length; index += 1) {
    if (left.visibleColumns[index] !== right.visibleColumns[index]) {
      return false;
    }
  }

  const leftWidthKeys = Object.keys(left.columnWidths).sort();
  const rightWidthKeys = Object.keys(right.columnWidths).sort();

  if (leftWidthKeys.length !== rightWidthKeys.length) {
    return false;
  }

  for (let index = 0; index < leftWidthKeys.length; index += 1) {
    const key = leftWidthKeys[index];
    if (key !== rightWidthKeys[index]) {
      return false;
    }
    if (left.columnWidths[key] !== right.columnWidths[key]) {
      return false;
    }
  }

  return true;
}

function stringListsEqual(left: string[], right: string[]): boolean {
  if (left.length !== right.length) {
    return false;
  }

  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }

  return true;
}

function resolveContextMenuPosition(
  container: HTMLElement | null,
  clientX: number,
  clientY: number,
  menuWidth: number,
) {
  if (!container) {
    return { x: clientX, y: clientY };
  }

  const bounds = container.getBoundingClientRect();
  const padding = 12;
  const localX = clientX - bounds.left + container.scrollLeft;
  const localY = clientY - bounds.top + container.scrollTop;
  const minLeft = container.scrollLeft + padding;
  const maxLeft = Math.max(
    minLeft,
    container.scrollLeft + container.clientWidth - menuWidth - padding,
  );

  return {
    x: Math.max(minLeft, Math.min(localX, maxLeft)),
    y: Math.max(container.scrollTop + padding, localY),
  };
}

function pickRandomTracks(tracks: ScannedTrack[], limit: number) {
  const shuffled = [...tracks];
  for (let index = shuffled.length - 1; index > 0; index -= 1) {
    const swapIndex = Math.floor(Math.random() * (index + 1));
    [shuffled[index], shuffled[swapIndex]] = [shuffled[swapIndex], shuffled[index]];
  }
  return shuffled.slice(0, limit);
}
