import { useEffect, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type { CatalogPatternRule } from '../../types/aria';

type CatalogRulesPanelProps = {
  rules: CatalogPatternRule[];
  onAddRule: () => void;
  onRemoveRule: (index: number) => void;
  onUpdateRule: (
    index: number,
    patch: Partial<CatalogPatternRule>,
  ) => void;
  onSave: () => void;
  variant?: 'card' | 'dialog';
};

export function CatalogRulesPanel({
  rules,
  onAddRule,
  onRemoveRule,
  onUpdateRule,
  onSave,
  variant = 'card',
}: CatalogRulesPanelProps) {
  const [composerDrafts, setComposerDrafts] = useState(() =>
    rules.map((rule) => rule.composers.join(', ')),
  );
  const [sourceTagDrafts, setSourceTagDrafts] = useState(() =>
    rules.map((rule) => rule.sourceTags.join(', ')),
  );

  useEffect(() => {
    setComposerDrafts(rules.map((rule) => rule.composers.join(', ')));
    setSourceTagDrafts(rules.map((rule) => rule.sourceTags.join(', ')));
  }, [rules.length]);

  const actions = (
    <div className="inline-actions">
      <button className="ghost-button" onClick={onAddRule} type="button">
        Add rule
      </button>
      <button onClick={onSave} type="button">
        Save rules
      </button>
    </div>
  );

  const content = (
    <>
      <p className="panel-copy">
        Add regex rules for catalog schemes like WAB, BWV, or composer-specific
        abbreviations. Composer hints are optional. Source tags are checked in
        descending priority.
      </p>

      <div className="mapping-list">
        {rules.map((rule, index) => (
          <article className="mapping-card" key={`${rule.label}-${index}`}>
            <div className="mapping-card__header">
              <strong>{rule.label || 'Custom rule'}</strong>
              <button
                className="ghost-button"
                onClick={() => onRemoveRule(index)}
                type="button"
              >
                Remove
              </button>
            </div>

            <label className="field-label">
              Rule label
              <input
                value={rule.label}
                onChange={(event) =>
                  onUpdateRule(index, { label: event.target.value })
                }
                placeholder="WAB"
              />
            </label>

            <label className="field-label">
              Regex pattern
              <input
                value={rule.pattern}
                onChange={(event) =>
                  onUpdateRule(index, { pattern: event.target.value })
                }
                placeholder="(?i)\\bWAB\\s*\\d+[A-Za-z]?\\b"
              />
            </label>

            <label className="field-label">
              Composer hints
              <input
                value={composerDrafts[index] ?? rule.composers.join(', ')}
                onBlur={() =>
                  setComposerDrafts((current) =>
                    updateDraftAtIndex(current, index, rule.composers.join(', ')),
                  )
                }
                onChange={(event) => {
                  const nextValue = event.target.value;
                  setComposerDrafts((current) =>
                    updateDraftAtIndex(current, index, nextValue),
                  );
                  onUpdateRule(index, {
                    composers: parseCommaSeparatedValues(nextValue),
                  });
                }}
                placeholder="Anton Bruckner, Bruckner"
              />
            </label>

            <label className="field-label">
              Source tags
              <input
                value={sourceTagDrafts[index] ?? rule.sourceTags.join(', ')}
                onBlur={() =>
                  setSourceTagDrafts((current) =>
                    updateDraftAtIndex(current, index, rule.sourceTags.join(', ')),
                  )
                }
                onChange={(event) => {
                  const nextValue = event.target.value;
                  setSourceTagDrafts((current) =>
                    updateDraftAtIndex(current, index, nextValue),
                  );
                  onUpdateRule(index, {
                    sourceTags: parseCommaSeparatedValues(nextValue),
                  });
                }}
                placeholder="TITLE, WORK, ALBUM"
              />
            </label>

            <label className="checkbox-row">
              <input
                checked={rule.enabled}
                onChange={(event) =>
                  onUpdateRule(index, { enabled: event.target.checked })
                }
                type="checkbox"
              />
              Enabled during catalog fallback
            </label>
          </article>
        ))}
      </div>
    </>
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
      eyebrow="Catalog Rules"
      title="Composer-aware catalog extraction"
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
