import type { ScannedTrack, TrackRawTags } from '../types/aria';

type TrackRawTagsDialogProps = {
  error: string | null;
  isLoading: boolean;
  isOpen: boolean;
  onClose: () => void;
  tags: TrackRawTags;
  track: ScannedTrack | null;
};

export function TrackRawTagsDialog({
  error,
  isLoading,
  isOpen,
  onClose,
  tags,
  track,
}: TrackRawTagsDialogProps) {
  if (!isOpen) {
    return null;
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="track-tags-dialog-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Track</p>
            <h3 id="track-tags-dialog-title">Raw file tags</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">{track?.fileName ?? 'Selected track'}</p>

        {track ? <p className="tag-inspector__path">{track.path}</p> : null}

        {isLoading ? (
          <div className="placeholder-pane">
            <strong>Reading tags from disk…</strong>
            <p>Aria is opening the file and extracting raw tags directly.</p>
          </div>
        ) : error ? (
          <div className="placeholder-pane">
            <strong>Could not read raw tags</strong>
            <p>{error}</p>
          </div>
        ) : (
          <div className="tag-inspector__list">
            {Object.entries(tags)
              .sort(([left], [right]) => left.localeCompare(right))
              .map(([tag, values]) => (
                <section className="tag-inspector__item" key={tag}>
                  <div className="tag-inspector__header">
                    <h4>{tag}</h4>
                    <span className="pane-chip">
                      {values.length} value{values.length === 1 ? '' : 's'}
                    </span>
                  </div>
                  <div className="tag-inspector__values">
                    {values.map((value, index) => (
                      <div className="tag-inspector__value" key={`${tag}-${index}`}>
                        <textarea
                          className="tag-inspector__textarea"
                          onFocus={(event) => event.currentTarget.select()}
                          readOnly
                          rows={Math.max(1, Math.min(4, value.split('\n').length || 1))}
                          value={value}
                        />
                        <button
                          className="ghost-button"
                          onClick={() => void copyToClipboard(value)}
                          type="button"
                        >
                          Copy
                        </button>
                      </div>
                    ))}
                  </div>
                </section>
              ))}
          </div>
        )}
      </div>
    </div>
  );
}

async function copyToClipboard(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    return;
  } catch {
    const textarea = document.createElement('textarea');
    textarea.value = value;
    textarea.setAttribute('readonly', 'true');
    textarea.style.position = 'absolute';
    textarea.style.left = '-9999px';
    document.body.appendChild(textarea);
    textarea.select();
    document.execCommand('copy');
    document.body.removeChild(textarea);
  }
}
