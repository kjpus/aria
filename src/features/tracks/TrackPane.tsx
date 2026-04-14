import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from 'react';
import { Fragment, useEffect, useMemo, useRef, useState } from 'react';
import { ClearableInput } from '../../components/ClearableInput';
import { SectionCard } from '../../components/SectionCard';
import { TrackRawTagsDialog } from '../../components/TrackRawTagsDialog';
import { readTrackRawTags } from '../../lib/aria';
import type {
  LibraryFieldMapping,
  ScannedTrack,
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
  getTrackColumnValue,
} from '../library/view-models';
import { HoverScrollText } from '../albums/HoverScrollText';

type TrackPaneProps = {
  tracks: ScannedTrack[];
  mappings: LibraryFieldMapping[];
  settings: TrackTableSettings;
  onPlayTracks: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddToQueue: (tracks: ScannedTrack[]) => void | Promise<void>;
  onAddToPlaylist: (tracks: ScannedTrack[]) => void | Promise<void>;
  onOpenAlbum: (albumId: string) => void;
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

type DropPosition = 'before' | 'after';

type DropIndicator = {
  key: string;
  position: DropPosition;
};

type ResizeState = {
  key: string;
  startX: number;
  startWidth: number;
};

type TrackContextMenuState = {
  x: number;
  y: number;
  trackIds: string[];
  primaryTrackId: string;
};

type AlbumTrackGroup = {
  albumId: string;
  album: AlbumCardModel | null;
  tracks: ScannedTrack[];
};

export function TrackPane({
  tracks,
  mappings,
  onPlayTracks,
  onAddToQueue,
  onAddToPlaylist,
  onOpenAlbum,
  onShowInExplorer,
  settings,
  onTrackTableChange,
}: TrackPaneProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [isLayoutDialogOpen, setIsLayoutDialogOpen] = useState(false);
  const [filter, setFilter] = useState('');
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
  const [selectedTrackIds, setSelectedTrackIds] = useState<string[]>([]);
  const [selectionAnchorId, setSelectionAnchorId] = useState<string | null>(null);
  const [draggedVisibleColumn, setDraggedVisibleColumn] = useState<string | null>(
    null,
  );
  const [visibleDropIndicator, setVisibleDropIndicator] =
    useState<DropIndicator | null>(null);
  const [resizingColumn, setResizingColumn] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<TrackContextMenuState | null>(null);
  const [tagInspectorTrack, setTagInspectorTrack] = useState<ScannedTrack | null>(null);
  const [tagInspectorTags, setTagInspectorTags] = useState<TrackRawTags>({});
  const [tagInspectorError, setTagInspectorError] = useState<string | null>(null);
  const [isTagInspectorOpen, setIsTagInspectorOpen] = useState(false);
  const [isTagInspectorLoading, setIsTagInspectorLoading] = useState(false);
  const [hasHydrated, setHasHydrated] = useState(false);
  const resizeStateRef = useRef<ResizeState | null>(null);
  const visibleDropIndicatorRef = useRef<DropIndicator | null>(null);
  const persistedTrackTable = useMemo(
    () => normalizeTrackTableSettings(settings, columns),
    [columns, settings],
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

      const nextWidth = clampColumnWidth(
        resizeState.key,
        resizeState.startWidth + (event.clientX - resizeState.startX),
      );

      setColumnWidths((current) => ({
        ...current,
        [resizeState.key]: nextWidth,
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
    if (!isLayoutDialogOpen && !isTagInspectorOpen) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsLayoutDialogOpen(false);
        setIsTagInspectorOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isLayoutDialogOpen, isTagInspectorOpen]);

  useEffect(() => {
    if (!contextMenu) {
      return undefined;
    }

    const closeMenu = (event?: Event) => {
      if (
        event instanceof MouseEvent &&
        menuRef.current?.contains(event.target as Node)
      ) {
        return;
      }

      if (event instanceof KeyboardEvent && event.key !== 'Escape') {
        return;
      }

      setContextMenu(null);
    };

    window.addEventListener('pointerdown', closeMenu);
    window.addEventListener('resize', closeMenu);
    window.addEventListener('keydown', closeMenu);
    window.addEventListener('scroll', closeMenu, true);

    return () => {
      window.removeEventListener('pointerdown', closeMenu);
      window.removeEventListener('resize', closeMenu);
      window.removeEventListener('keydown', closeMenu);
      window.removeEventListener('scroll', closeMenu, true);
    };
  }, [contextMenu]);

  const sortSummary = useMemo(
    () => describeSortCriteria(sortCriteria, columnLookup),
    [columnLookup, sortCriteria],
  );

  const filteredTracks = useMemo(() => {
    const query = filter.trim().toLowerCase();
    const nextTracks = tracks.filter((track) => {
      if (!query) {
        return true;
      }

      const searchable = [
        track.fileName,
        track.path,
        ...Object.values(track.mappedFields).flat(),
      ]
        .join(' ')
        .toLowerCase();

      return searchable.includes(query);
    });

    return [...nextTracks].sort((left, right) =>
      compareTracks(left, right, sortCriteria),
    );
  }, [filter, sortCriteria, tracks]);
  const albumCards = useMemo(
    () => new Map(buildAlbumCards(filteredTracks).map((album) => [album.id, album])),
    [filteredTracks],
  );
  const albumGroups = useMemo(
    () => buildAlbumGroups(filteredTracks, albumCards),
    [albumCards, filteredTracks],
  );

  useEffect(() => {
    const availableIds = new Set(filteredTracks.map((track) => track.id));

    setSelectedTrackIds((current) => {
      const next = current.filter((trackId) => availableIds.has(trackId));
      return stringListsEqual(current, next) ? current : next;
    });

    setSelectionAnchorId((current) =>
      current && availableIds.has(current) ? current : null,
    );
  }, [filteredTracks]);

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
  ) {
    event.preventDefault();
    event.stopPropagation();

    resizeStateRef.current = {
      key,
      startX: event.clientX,
      startWidth: columnWidths[key] ?? getDefaultColumnWidth(key),
    };
    setResizingColumn(key);
  }

  function handleTrackClick(
    event: ReactMouseEvent<HTMLTableRowElement>,
    trackId: string,
  ) {
    const orderedIds = filteredTracks.map((track) => track.id);

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

  function handleTrackDoubleClick(track: ScannedTrack) {
    const selectedTracks = selectedTrackSet.has(track.id)
      ? selectTracksInOrder(filteredTracks, selectedTrackIds)
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
      ? selectTrackIdsInOrder(filteredTracks, selectedTrackIds)
      : [track.id];

    if (!selectedTrackSet.has(track.id)) {
      setSelectedTrackIds(nextSelectedTrackIds);
      setSelectionAnchorId(track.id);
    }

    setContextMenu({
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 180),
      trackIds: nextSelectedTrackIds,
      primaryTrackId: track.id,
    });
  }

  async function runTrackContextAction(
    action: (tracks: ScannedTrack[]) => void | Promise<void>,
  ) {
    if (!contextMenu) {
      return;
    }

    const selectedTracks = selectTracksInOrder(filteredTracks, contextMenu.trackIds);
    if (selectedTracks.length === 0) {
      setContextMenu(null);
      return;
    }

    setContextMenu(null);
    await action(selectedTracks);
  }

  async function runShowInExplorer() {
    if (!contextMenu || contextMenu.trackIds.length !== 1) {
      return;
    }

    const track = filteredTracks.find(
      (candidate) => candidate.id === contextMenu.primaryTrackId,
    );

    if (!track) {
      setContextMenu(null);
      return;
    }

    setContextMenu(null);
    await onShowInExplorer(track);
  }

  async function runShowAllTags() {
    if (!contextMenu || contextMenu.trackIds.length !== 1) {
      return;
    }

    const track = filteredTracks.find(
      (candidate) => candidate.id === contextMenu.primaryTrackId,
    );

    if (!track) {
      setContextMenu(null);
      return;
    }

    setContextMenu(null);
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

  return (
    <div className="pane-stack">
      <SectionCard hideHeader>
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
            </div>
            <button
              className="ghost-button"
              onClick={() => setIsLayoutDialogOpen(true)}
              type="button"
            >
              Layout & Sort
            </button>
          </div>
        </div>

        {filteredTracks.length === 0 ? (
          <div className="placeholder-pane">
            <strong>No tracks match the current filters</strong>
            <p>Adjust the filter text to bring albums back into view.</p>
          </div>
        ) : (
          <div className="track-table-shell track-table-shell--clamped">
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
                  {visibleColumns.map((key) => {
                    const column = columnLookup.get(key);

                    return (
                      <th className="track-table__header" key={key}>
                        <div className="track-table__header-inner">
                          <span>{column?.label ?? key}</span>
                          <div
                            className="track-table__resize-handle"
                            onPointerDown={(event) => handleResizeStart(event, key)}
                            role="presentation"
                          />
                        </div>
                      </th>
                    );
                  })}
                </tr>
              </thead>
              <tbody>
                {albumGroups.map((group) => (
                  <Fragment key={group.albumId}>
                    <tr
                      className="track-table__album-row"
                      onClick={() => onOpenAlbum(group.albumId)}
                    >
                      <td colSpan={Math.max(visibleColumns.length, 1)}>
                        <div className="track-table__album-meta">
                          <HoverScrollText
                            className="track-table__album-title"
                            speed={44}
                            text={
                              group.album?.title ||
                              group.tracks[0]?.mappedFields.album?.[0] ||
                              'Unknown album'
                            }
                          />
                        </div>
                      </td>
                    </tr>
                    {group.tracks.map((track) => (
                      <tr
                        aria-selected={selectedTrackSet.has(track.id)}
                        className={[
                          'track-table__row--playable',
                          selectedTrackSet.has(track.id) ? 'track-table__row--selected' : '',
                        ]
                          .filter(Boolean)
                          .join(' ')}
                        key={track.id}
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
                    ))}
                  </Fragment>
                ))}
              </tbody>
            </table>
          </div>
        )}

        {contextMenu ? (
          <div
            className="album-context-menu"
            ref={menuRef}
            style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
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
            {contextMenu.trackIds.length === 1 ? (
              <button onClick={() => void runShowAllTags()} type="button">
                Show all tags
              </button>
            ) : null}
            {contextMenu.trackIds.length === 1 ? (
              <button onClick={() => void runShowInExplorer()} type="button">
                Show in Explorer
              </button>
            ) : null}
          </div>
        ) : null}
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
                  Tracks are sorted by the first rule, then ties fall through to
                  the next rules in order.
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
    tracks: albumTracks,
  }));
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
  const min =
    key === 'path'
      ? 220
      : key === 'title' || key === 'album'
        ? 160
        : 96;
  const max = key === 'path' ? 720 : 520;
  return Math.max(min, Math.min(max, width));
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
