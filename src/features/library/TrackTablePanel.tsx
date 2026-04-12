import { convertFileSrc } from '@tauri-apps/api/core';
import { SectionCard } from '../../components/SectionCard';
import type { LibraryFieldMapping, ScannedTrack } from '../../types/aria';

type TrackTablePanelProps = {
  tracks: ScannedTrack[];
  mappings: LibraryFieldMapping[];
};

function imageSrc(path: string | null) {
  return path ? convertFileSrc(path) : null;
}

export function TrackTablePanel({ tracks, mappings }: TrackTablePanelProps) {
  const visibleMappings = mappings.filter((mapping) => mapping.key && mapping.label);

  return (
    <SectionCard eyebrow="Tracks" title="Scanned track list">
      <div className="track-table-shell">
        <table className="track-table">
          <thead>
            <tr>
              <th>Art</th>
              {visibleMappings.map((mapping) => (
                <th key={mapping.key}>{mapping.label}</th>
              ))}
              <th>Format</th>
              <th>Path</th>
            </tr>
          </thead>
          <tbody>
            {tracks.length === 0 ? (
              <tr>
                <td className="empty-state" colSpan={visibleMappings.length + 3}>
                  No tracks scanned yet.
                </td>
              </tr>
            ) : (
              tracks.map((track) => {
                const art = imageSrc(track.albumArtPath);

                return (
                  <tr key={track.id}>
                    <td>
                      {art ? (
                        <img
                          alt=""
                          className="track-thumb"
                          src={art}
                        />
                      ) : (
                        <div className="track-thumb track-thumb--empty">Aria</div>
                      )}
                    </td>
                    {visibleMappings.map((mapping) => (
                      <td key={`${track.id}-${mapping.key}`}>
                        {(track.mappedFields[mapping.key] ?? []).join(' • ')}
                      </td>
                    ))}
                    <td>{track.audio.format}</td>
                    <td className="track-path">{track.path}</td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </SectionCard>
  );
}
