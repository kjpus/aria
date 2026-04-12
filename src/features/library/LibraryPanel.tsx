import { SectionCard } from '../../components/SectionCard';
import type { LibrarySnapshot } from '../../types/aria';

type LibraryPanelProps = {
  library: LibrarySnapshot;
  onAddDirectory: () => void;
  onClearLibrary: () => void;
  onRemoveRoot: (path: string) => void;
  onRescanAll: () => void;
};

export function LibraryPanel({
  library,
  onAddDirectory,
  onClearLibrary,
  onRemoveRoot,
  onRescanAll,
}: LibraryPanelProps) {
  return (
    <SectionCard eyebrow="Library" title="Classical collection">
      <div className="inline-actions library-panel__actions">
        <button disabled={library.isScanning} onClick={onAddDirectory} type="button">
          Add directory
        </button>
        <button
          className="ghost-button"
          disabled={library.isScanning || library.roots.length === 0}
          onClick={onRescanAll}
          type="button"
        >
          {library.isScanning ? 'Scanning...' : 'Rescan all'}
        </button>
        <button
          className="ghost-button ghost-button--danger"
          disabled={library.isScanning || (library.roots.length === 0 && library.tracks.length === 0)}
          onClick={onClearLibrary}
          type="button"
        >
          Clear library
        </button>
      </div>

      <div className="metrics-grid">
        <div>
          <span>Directories</span>
          <strong>{library.roots.length}</strong>
        </div>
        <div>
          <span>Albums</span>
          <strong>
            {
              new Set(
                library.tracks.map((track) => track.mappedFields.album?.[0] ?? track.path),
              ).size
            }
          </strong>
        </div>
        <div>
          <span>Tracks</span>
          <strong>{library.tracks.length}</strong>
        </div>
      </div>

      <ul className="root-list root-list--directories">
        {library.roots.length === 0 ? (
          <li className="empty-state">No library directories yet.</li>
        ) : (
          library.roots.map((root) => (
            <li key={root.path}>
              <span>{root.path}</span>
              <button
                className="ghost-button"
                disabled={library.isScanning}
                onClick={() => onRemoveRoot(root.path)}
                type="button"
              >
                Remove
              </button>
            </li>
          ))
        )}
      </ul>
    </SectionCard>
  );
}
