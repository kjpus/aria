import { useEffect, useMemo, useState } from 'react';
import type { LibraryFieldMapping, ScannedTrack } from '../types/aria';

type ExportFieldDialogProps = {
  error: string | null;
  fieldMappings: LibraryFieldMapping[];
  isOpen: boolean;
  isSubmitting: boolean;
  onAddTagOption: (tagName: string) => void;
  onClose: () => void;
  onSubmit: (fieldKey: string, tagName: string) => void | Promise<void>;
  sessionTagOptions: string[];
  tracks: ScannedTrack[];
};

type FieldOption = {
  key: string;
  label: string;
  populatedCount: number;
  previewValues: string[];
};

type TagOverwriteWarning = {
  populatedCount: number;
  previewValues: string[];
};

export function ExportFieldDialog({
  error,
  fieldMappings,
  isOpen,
  isSubmitting,
  onAddTagOption,
  onClose,
  onSubmit,
  sessionTagOptions,
  tracks,
}: ExportFieldDialogProps) {
  const fieldOptions = useMemo(
    () => buildFieldOptions(fieldMappings, tracks),
    [fieldMappings, tracks],
  );
  const tagOptions = useMemo(
    () => buildTagOptions(tracks, sessionTagOptions),
    [sessionTagOptions, tracks],
  );
  const [selectedFieldKey, setSelectedFieldKey] = useState('');
  const [selectedTagName, setSelectedTagName] = useState('');
  const [customTagName, setCustomTagName] = useState('');
  const [isAddingTag, setIsAddingTag] = useState(false);

  useEffect(() => {
    if (!isOpen) {
      return;
    }

    setSelectedFieldKey((current) =>
      fieldOptions.some((field) => field.key === current)
        ? current
        : fieldOptions[0]?.key ?? '',
    );
    setSelectedTagName((current) =>
      current.trim() || tagOptions[0] || '',
    );
    setCustomTagName('');
    setIsAddingTag(false);
  }, [fieldOptions, isOpen, tagOptions]);

  const selectedField =
    fieldOptions.find((field) => field.key === selectedFieldKey) ?? null;
  const tagOverwriteWarning = useMemo(
    () => buildTagOverwriteWarning(selectedTagName, tracks),
    [selectedTagName, tracks],
  );
  const displayTagOptions = useMemo(() => {
    const tags = [...tagOptions];

    if (selectedTagName && !tags.includes(selectedTagName)) {
      tags.unshift(selectedTagName);
    }

    return tags;
  }, [selectedTagName, tagOptions]);

  if (!isOpen) {
    return null;
  }

  const canSubmit =
    selectedFieldKey.trim().length > 0 && selectedTagName.trim().length > 0;

  function handleAddTag() {
    const nextTagName = normalizeTagName(customTagName);
    if (!nextTagName) {
      return;
    }

    onAddTagOption(nextTagName);
    setSelectedTagName(nextTagName);
    setCustomTagName('');
    setIsAddingTag(false);
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="export-field-dialog-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Tags</p>
            <h3 id="export-field-dialog-title">Export field</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">
          Write one mapped field into a raw media-file tag for {tracks.length}{' '}
          track{tracks.length === 1 ? '' : 's'}. When a track has no value for the
          selected field, that tag is cleared for that file.
        </p>

        <div className="export-field-dialog__summary">
          <span className="pane-chip">
            {tracks.length} file{tracks.length === 1 ? '' : 's'}
          </span>
          {selectedField ? (
            <span className="pane-chip">
              {selectedField.populatedCount}/{tracks.length} populated
            </span>
          ) : null}
        </div>

        {tagOverwriteWarning ? (
          <div className="export-field-dialog__warning">
            <strong>Warning</strong>
            <p>
              {selectedTagName} already has values in{' '}
              {tagOverwriteWarning.populatedCount} of {tracks.length} selected{' '}
              file{tracks.length === 1 ? '' : 's'}. Exporting will overwrite the
              existing tag value{tagOverwriteWarning.populatedCount === 1 ? '' : 's'}.
            </p>
            {tagOverwriteWarning.previewValues.length > 0 ? (
              <p>
                Existing values: {tagOverwriteWarning.previewValues.join(' / ')}
              </p>
            ) : null}
          </div>
        ) : null}

        <div className="export-field-dialog__grid">
          <section className="export-field-dialog__column">
            <div className="dialog-section__header">
              <h4>Fields</h4>
            </div>
            <div className="export-field-dialog__list">
              {fieldOptions.map((field) => (
                <button
                  className={[
                    'export-field-dialog__option',
                    selectedFieldKey === field.key
                      ? 'export-field-dialog__option--selected'
                      : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                  key={field.key}
                  onClick={() => setSelectedFieldKey(field.key)}
                  type="button"
                >
                  <span className="export-field-dialog__option-head">
                    <strong>{field.label}</strong>
                    <span className="pane-chip">
                      {field.populatedCount}/{tracks.length}
                    </span>
                  </span>
                  <span className="export-field-dialog__option-meta">{field.key}</span>
                  <span className="export-field-dialog__option-preview">
                    {field.previewValues.join(' / ') || 'No values in current selection'}
                  </span>
                </button>
              ))}
            </div>
          </section>

          <section className="export-field-dialog__column">
            <div className="dialog-section__header">
              <h4>Tags</h4>
              <button
                className="ghost-button"
                onClick={() => setIsAddingTag((current) => !current)}
                type="button"
              >
                Add tag
              </button>
            </div>

            {isAddingTag ? (
              <div className="export-field-dialog__new-tag">
                <label className="field-label">
                  New tag
                  <input
                    onChange={(event) => setCustomTagName(event.target.value)}
                    placeholder="CATALOGNUMBER"
                    value={customTagName}
                  />
                </label>
                <div className="inline-actions">
                  <button
                    className="ghost-button"
                    onClick={() => {
                      setCustomTagName('');
                      setIsAddingTag(false);
                    }}
                    type="button"
                  >
                    Cancel
                  </button>
                  <button onClick={handleAddTag} type="button">
                    Use tag
                  </button>
                </div>
              </div>
            ) : null}

            <div className="export-field-dialog__list">
              {displayTagOptions.length > 0 ? (
                displayTagOptions.map((tag) => (
                  <button
                    className={[
                      'export-field-dialog__option',
                      selectedTagName === tag
                        ? 'export-field-dialog__option--selected'
                        : '',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    key={tag}
                    onClick={() => setSelectedTagName(tag)}
                    type="button"
                  >
                    <strong>{tag}</strong>
                  </button>
                ))
              ) : (
                <div className="placeholder-pane">
                  <strong>No existing tags in this selection</strong>
                  <p>Use Add tag to create a new target tag.</p>
                </div>
              )}
            </div>
          </section>
        </div>

        {error ? <div className="export-field-dialog__error">{error}</div> : null}

        <div className="export-field-dialog__actions">
          <button className="ghost-button" onClick={onClose} type="button">
            Cancel
          </button>
          <button
            disabled={!canSubmit || isSubmitting}
            onClick={() => void onSubmit(selectedFieldKey, selectedTagName)}
            type="button"
          >
            {isSubmitting ? 'Exporting...' : 'Export'}
          </button>
        </div>
      </div>
    </div>
  );
}

function buildFieldOptions(
  fieldMappings: LibraryFieldMapping[],
  tracks: ScannedTrack[],
): FieldOption[] {
  const mappingLookup = new Map(
    fieldMappings.map((mapping) => [mapping.key, mapping.label]),
  );
  const keys = new Set(fieldMappings.map((mapping) => mapping.key));

  for (const track of tracks) {
    for (const key of Object.keys(track.mappedFields)) {
      keys.add(key);
    }
  }

  return Array.from(keys)
    .map((key) => {
      const previewValues: string[] = [];
      let populatedCount = 0;

      for (const track of tracks) {
        const values = track.mappedFields[key] ?? [];
        if (values.length === 0) {
          continue;
        }

        populatedCount += 1;
        const preview = values.join(' / ');
        if (preview && !previewValues.includes(preview) && previewValues.length < 3) {
          previewValues.push(preview);
        }
      }

      return {
        key,
        label: mappingLookup.get(key) ?? humanizeFieldKey(key),
        populatedCount,
        previewValues,
      };
    })
    .sort((left, right) => left.label.localeCompare(right.label));
}

function buildTagOptions(
  tracks: ScannedTrack[],
  sessionTagOptions: string[],
): string[] {
  const tags = new Map<string, string>();

  for (const tag of sessionTagOptions) {
    addTagOption(tags, tag);
  }

  for (const track of tracks) {
    for (const tag of Object.keys(track.rawTags)) {
      addTagOption(tags, tag);
    }
  }

  return Array.from(tags.values()).sort((left, right) =>
    left.localeCompare(right),
  );
}

function addTagOption(tags: Map<string, string>, tagName: string) {
  const normalized = normalizeTagName(tagName);
  if (!normalized || tags.has(normalized)) {
    return;
  }

  tags.set(normalized, normalized);
}

function normalizeTagName(tagName: string): string {
  return tagName.trim().toUpperCase();
}

function buildTagOverwriteWarning(
  tagName: string,
  tracks: ScannedTrack[],
): TagOverwriteWarning | null {
  const normalizedTagName = normalizeTagName(tagName);
  if (!normalizedTagName) {
    return null;
  }

  let populatedCount = 0;
  const previewValues: string[] = [];

  for (const track of tracks) {
    const values = (track.rawTags[normalizedTagName] ?? [])
      .map((value) => value.trim())
      .filter(Boolean);

    if (values.length === 0) {
      continue;
    }

    populatedCount += 1;
    const preview = values.join(' / ');
    if (preview && !previewValues.includes(preview) && previewValues.length < 3) {
      previewValues.push(preview);
    }
  }

  if (populatedCount === 0) {
    return null;
  }

  return {
    populatedCount,
    previewValues,
  };
}

function humanizeFieldKey(key: string): string {
  return key
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}
