import { useEffect, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type { LibraryFieldMapping } from '../../types/aria';

type FieldMappingsPanelProps = {
  mappings: LibraryFieldMapping[];
  onAddField: () => void;
  onRemoveField: (index: number) => void;
  onUpdateField: (
    index: number,
    patch: Partial<LibraryFieldMapping>,
  ) => void;
  onSave: () => void;
  variant?: 'card' | 'dialog';
};

export function FieldMappingsPanel({
  mappings,
  onAddField,
  onRemoveField,
  onUpdateField,
  onSave,
  variant = 'card',
}: FieldMappingsPanelProps) {
  const [tagPriorityDrafts, setTagPriorityDrafts] = useState(() =>
    mappings.map((mapping) => mapping.tagPriorities.join(', ')),
  );

  useEffect(() => {
    setTagPriorityDrafts(mappings.map((mapping) => mapping.tagPriorities.join(', ')));
  }, [mappings.length]);

  const actions = (
    <div className="inline-actions">
      <button className="ghost-button" onClick={onAddField} type="button">
        Add field
      </button>
      <button onClick={onSave} type="button">
        Save mappings
      </button>
    </div>
  );

  const content = (
    <div className="mapping-list">
      {mappings.map((mapping, index) => (
        <article className="mapping-card" key={`${mapping.key}-${index}`}>
          <div className="mapping-card__header">
            <strong>{mapping.label || 'New field'}</strong>
            <button
              className="ghost-button"
              onClick={() => onRemoveField(index)}
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
                onUpdateField(index, { key: event.target.value })
              }
              placeholder="catalog"
            />
          </label>

          <label className="field-label">
            Display label
            <input
              value={mapping.label}
              onChange={(event) =>
                onUpdateField(index, { label: event.target.value })
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
                onUpdateField(index, {
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
