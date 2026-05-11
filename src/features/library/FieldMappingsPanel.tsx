import { useEffect, useMemo, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type { LibraryFieldMapping } from '../../types/aria';
import {
  FIELD_MAPPING_FORMAT_OPTIONS,
  type FieldMappingFormat,
  fieldMappingFormatLabel,
  normalizeFieldMappingFormat,
} from '../../lib/field-mapping-presets';

type FieldMappingsPanelProps = {
  mappings: LibraryFieldMapping[];
  selectedFormat: FieldMappingFormat;
  onAddField: (format: FieldMappingFormat) => void;
  onRemoveField: (format: FieldMappingFormat, index: number) => void;
  onSelectFormat: (format: FieldMappingFormat) => void;
  onUpdateField: (
    format: FieldMappingFormat,
    index: number,
    patch: Partial<LibraryFieldMapping>,
  ) => void;
  onSave: () => void;
  variant?: 'card' | 'dialog';
};

export function FieldMappingsPanel({
  mappings,
  selectedFormat,
  onAddField,
  onRemoveField,
  onSelectFormat,
  onUpdateField,
  onSave,
  variant = 'card',
}: FieldMappingsPanelProps) {
  const visibleMappings = useMemo(
    () =>
      mappings.filter(
        (mapping) => normalizeFieldMappingFormat(mapping.format) === selectedFormat,
      ),
    [mappings, selectedFormat],
  );

  const [tagPriorityDrafts, setTagPriorityDrafts] = useState(() =>
    visibleMappings.map((mapping) => mapping.tagPriorities.join(', ')),
  );

  useEffect(() => {
    setTagPriorityDrafts(visibleMappings.map((mapping) => mapping.tagPriorities.join(', ')));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedFormat, visibleMappings.length]);

  const actions = (
    <div className="inline-actions">
      <label className="field-label">
        File format
        <select
          value={selectedFormat}
          onChange={(event) =>
            onSelectFormat(normalizeFieldMappingFormat(event.target.value))
          }
        >
          {FIELD_MAPPING_FORMAT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <button
        className="ghost-button"
        onClick={() => onAddField(selectedFormat)}
        type="button"
      >
        Add field
      </button>
      <button onClick={onSave} type="button">
        Save mappings
      </button>
    </div>
  );

  const content = (
    <div className="mapping-list">
      {visibleMappings.length === 0 ? (
        <p className="panel-copy">
          No fields configured for {fieldMappingFormatLabel(selectedFormat)} yet.
        </p>
      ) : null}
      {visibleMappings.map((mapping, index) => (
        <article className="mapping-card" key={`${mapping.key}-${index}`}>
          <div className="mapping-card__header">
            <strong>{mapping.label || 'New field'}</strong>
            <button
              className="ghost-button"
              onClick={() => onRemoveField(selectedFormat, index)}
              type="button"
            >
              Remove
            </button>
          </div>

          <label className="field-label">
            Field key
            <input
              value={mapping.key}
              onChange={(event) =>
                onUpdateField(selectedFormat, index, { key: event.target.value })
              }
              placeholder="catalog"
            />
          </label>

          <label className="field-label">
            Display label
            <input
              value={mapping.label}
              onChange={(event) =>
                onUpdateField(selectedFormat, index, { label: event.target.value })
              }
              placeholder="Catalog"
            />
          </label>

          <label className="field-label">
            Tag priorities
            <input
              value={tagPriorityDrafts[index] ?? mapping.tagPriorities.join(', ')}
              onBlur={() =>
                setTagPriorityDrafts((current) =>
                  updateDraftAtIndex(current, index, mapping.tagPriorities.join(', ')),
                )
              }
              onChange={(event) => {
                const nextValue = event.target.value;
                setTagPriorityDrafts((current) =>
                  updateDraftAtIndex(current, index, nextValue),
                );
                onUpdateField(selectedFormat, index, {
                  tagPriorities: parseCommaSeparatedValues(nextValue),
                });
              }}
              placeholder="PERFORMER, ARTIST, ALBUMARTIST"
            />
          </label>
        </article>
      ))}
    </div>
  );

  if (variant === 'dialog') {
    return (
      <div className="editor-shell">
        <div className="editor-shell__toolbar">{actions}</div>
        {content}
      </div>
    );
  }

  return (
    <SectionCard
      eyebrow="Mappings"
      title="Database fields and tag priorities"
      actions={actions}
    >
      {content}
    </SectionCard>
  );
}

function parseCommaSeparatedValues(value: string) {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function updateDraftAtIndex(current: string[], index: number, value: string) {
  const next = [...current];
  next[index] = value;
  return next;
}
