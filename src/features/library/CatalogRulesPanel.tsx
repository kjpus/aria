import { useEffect, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type { CatalogRule } from '../../types/aria';

type CatalogRulesPanelProps = {
  rules: CatalogRule[];
  onAddRule: () => void;
  onRemoveRule: (index: number) => void;
  onUpdateRule: (
    index: number,
    patch: Partial<CatalogRule>,
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

  useEffect(() => {
    setComposerDrafts(rules.map((rule) => rule.composers.join(', ')));
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
        Define the catalog abbreviation Aria should look for. All rules use the
        same source-tag order and parsing flow; composer hints only control when
        a label applies. The default catch-all is <code>Op</code>.
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
                placeholder="BWV"
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
