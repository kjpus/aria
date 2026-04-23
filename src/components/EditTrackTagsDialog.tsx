import { useEffect, useMemo, useState } from 'react';
import type { ScannedTrack, TrackTagEditUpdate } from '../types/aria';

type EditTrackTagsDialogProps = {
  error: string | null;
  isOpen: boolean;
  isSubmitting: boolean;
  onClose: () => void;
  onSubmit: (updates: TrackTagEditUpdate[]) => void | Promise<void>;
  tracks: ScannedTrack[];
};

type EditableTagRow = {
  id: string;
  initialValueText: string;
  isNew: boolean;
  isVaries: boolean;
  clearRequested: boolean;
  tagName: string;
  valueText: string;
};

export function EditTrackTagsDialog({
  error,
  isOpen,
  isSubmitting,
  onClose,
  onSubmit,
  tracks,
}: EditTrackTagsDialogProps) {
  const [rows, setRows] = useState<EditableTagRow[]>([]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    setRows(buildEditableTagRows(tracks));
  }, [isOpen, tracks]);

  const pendingUpdates = useMemo(() => buildPendingUpdates(rows), [rows]);
  const validationError = useMemo(() => validateRows(rows), [rows]);
  const displayError = validationError ?? error;

  if (!isOpen) {
    return null;
  }

  function addTagRow() {
    setRows((current) => [
      ...current,
      {
        id: `new:${Date.now()}:${current.length}`,
        initialValueText: '',
        isNew: true,
        isVaries: false,
        clearRequested: false,
        tagName: '',
        valueText: '',
      },
    ]);
  }

  function updateTagName(rowId: string, tagName: string) {
    setRows((current) =>
      current.map((row) =>
        row.id === rowId
          ? {
              ...row,
              tagName,
            }
          : row,
      ),
    );
  }

  function updateValueText(rowId: string, valueText: string) {
    setRows((current) =>
      current.map((row) =>
        row.id === rowId
          ? {
              ...row,
              clearRequested: false,
              valueText,
            }
          : row,
      ),
    );
  }

  function toggleClearRequested(rowId: string) {
    setRows((current) =>
      current.map((row) =>
        row.id === rowId
          ? {
              ...row,
              clearRequested: !row.clearRequested,
              valueText: row.clearRequested ? row.initialValueText : '',
            }
          : row,
      ),
    );
  }

  function resetRow(rowId: string) {
    setRows((current) =>
      current.map((row) =>
        row.id === rowId
          ? {
              ...row,
              clearRequested: false,
              valueText: row.initialValueText,
            }
          : row,
      ),
    );
  }

  function removeNewRow(rowId: string) {
    setRows((current) => current.filter((row) => row.id !== rowId));
  }

  const canSubmit =
    pendingUpdates.length > 0 && displayError === null && !isSubmitting;

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="edit-track-tags-dialog-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Tags</p>
            <h3 id="edit-track-tags-dialog-title">Edit tags</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">
          Edit raw file tags for {tracks.length} selected track
          {tracks.length === 1 ? '' : 's'}. Use one value per line. Rows showing{' '}
          <code>&lt;varies&gt;</code> stay unchanged until you edit or clear them.
        </p>

        <div className="tag-editor__summary">
          <span className="pane-chip">
            {tracks.length} file{tracks.length === 1 ? '' : 's'}
          </span>
          <span className="pane-chip">
            {pendingUpdates.length} pending change
            {pendingUpdates.length === 1 ? '' : 's'}
          </span>
        </div>

        <div className="dialog-section__header">
          <h4>Tags</h4>
          <button className="ghost-button" onClick={addTagRow} type="button">
            Add tag
          </button>
        </div>

        {rows.length > 0 ? (
          <div className="tag-editor__list">
            {rows.map((row) => {
              const dirty = isRowDirty(row);
              const valueCount = parseTagValues(row.valueText).length;

              return (
                <section className="tag-inspector__item" key={row.id}>
                  <div className="tag-inspector__header">
                    {row.isNew ? (
                      <label className="field-label tag-editor__name">
                        Tag name
                        <input
                          className="tag-editor__name-input"
                          onChange={(event) =>
                            updateTagName(row.id, event.target.value)
                          }
                          placeholder="CATALOGNUMBER"
                          value={row.tagName}
                        />
                      </label>
                    ) : (
                      <div className="tag-editor__name">
                        <h4>{row.tagName}</h4>
                        <span className="pane-chip">
                          {row.isVaries
                            ? '<varies>'
                            : `${valueCount} value${valueCount === 1 ? '' : 's'}`}
                        </span>
                      </div>
                    )}

                    <div className="inline-actions">
                      {row.isNew ? (
                        <button
                          className="ghost-button"
                          onClick={() => removeNewRow(row.id)}
                          type="button"
                        >
                          Remove
                        </button>
                      ) : (
                        <>
                          {dirty ? (
                            <button
                              className="ghost-button"
                              onClick={() => resetRow(row.id)}
                              type="button"
                            >
                              Reset
                            </button>
                          ) : null}
                          <button
                            className="ghost-button"
                            onClick={() => toggleClearRequested(row.id)}
                            type="button"
                          >
                            {row.clearRequested ? 'Undo clear' : 'Clear'}
                          </button>
                        </>
                      )}
                    </div>
                  </div>

                  {row.clearRequested ? (
                    <p className="tag-editor__note">
                      This tag will be removed from all selected tracks.
                    </p>
                  ) : row.isVaries ? (
                    <p className="tag-editor__note">
                      Current values differ across the selection. Enter a new value
                      to replace all of them.
                    </p>
                  ) : null}

                  <textarea
                    className="tag-inspector__textarea"
                    onChange={(event) => updateValueText(row.id, event.target.value)}
                    placeholder={row.isVaries ? '<varies>' : 'One value per line'}
                    rows={Math.max(
                      3,
                      Math.min(6, row.valueText.split('\n').length + 1),
                    )}
                    value={row.valueText}
                  />
                </section>
              );
            })}
          </div>
        ) : (
          <div className="placeholder-pane">
            <strong>No existing tags in this selection</strong>
            <p>Use Add tag to create the first tag you want to write.</p>
          </div>
        )}

        {displayError ? (
          <div className="export-field-dialog__error">{displayError}</div>
        ) : null}

        <div className="export-field-dialog__actions">
          <button className="ghost-button" onClick={onClose} type="button">
            Cancel
          </button>
          <button
            disabled={!canSubmit}
            onClick={() => void onSubmit(pendingUpdates)}
            type="button"
          >
            {isSubmitting ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}

function buildEditableTagRows(tracks: ScannedTrack[]): EditableTagRow[] {
  const tagNames = new Set<string>();

  for (const track of tracks) {
    for (const tagName of Object.keys(track.rawTags)) {
      tagNames.add(normalizeTagName(tagName));
    }
  }

  return Array.from(tagNames)
    .sort((left, right) => left.localeCompare(right))
    .map((tagName) => {
      const valuesPerTrack = tracks.map((track) =>
        normalizeTagValues(track.rawTags[tagName] ?? []),
      );
      const sharedValues = valuesPerTrack.every((values) =>
        valueListsEqual(values, valuesPerTrack[0] ?? []),
      )
        ? valuesPerTrack[0] ?? []
        : null;
      const initialValueText = sharedValues ? serializeTagValues(sharedValues) : '';

      return {
        id: `existing:${tagName}`,
        initialValueText,
        isNew: false,
        isVaries: sharedValues === null,
        clearRequested: false,
        tagName,
        valueText: initialValueText,
      };
    });
}

function buildPendingUpdates(rows: EditableTagRow[]): TrackTagEditUpdate[] {
  return rows
    .filter((row) => isRowDirty(row))
    .map((row) => ({
      tagName: normalizeTagName(row.tagName),
      values: row.clearRequested ? [] : parseTagValues(row.valueText),
    }))
    .filter((row) => row.tagName.length > 0);
}

function validateRows(rows: EditableTagRow[]): string | null {
  const seen = new Set<string>();

  for (const row of rows) {
    const tagName = normalizeTagName(row.tagName);
    const hasValues = parseTagValues(row.valueText).length > 0;
    const shouldValidate = !row.isNew || hasValues;

    if (row.isNew && hasValues && !tagName) {
      return 'New tags need a name before they can be saved.';
    }

    if (!shouldValidate || !tagName) {
      continue;
    }

    if (seen.has(tagName)) {
      return `${tagName} is listed more than once.`;
    }

    seen.add(tagName);
  }

  return null;
}

function isRowDirty(row: EditableTagRow): boolean {
  if (row.isNew) {
    return (
      normalizeTagName(row.tagName).length > 0 && parseTagValues(row.valueText).length > 0
    );
  }

  if (row.clearRequested) {
    return true;
  }

  const currentValueText = serializeTagValues(parseTagValues(row.valueText));
  if (row.isVaries) {
    return currentValueText.length > 0;
  }

  return currentValueText !== row.initialValueText;
}

function parseTagValues(valueText: string): string[] {
  return normalizeTagValues(valueText.split(/\r?\n/));
}

function normalizeTagValues(values: string[]): string[] {
  const nextValues: string[] = [];

  for (const value of values) {
    const trimmed = value.trim();
    if (!trimmed || nextValues.includes(trimmed)) {
      continue;
    }

    nextValues.push(trimmed);
  }

  return nextValues;
}

function serializeTagValues(values: string[]): string {
  return values.join('\n');
}

function valueListsEqual(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index]);
}

function normalizeTagName(tagName: string): string {
  return tagName.trim().toUpperCase();
}
