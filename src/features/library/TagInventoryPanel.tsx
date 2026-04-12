import { SectionCard } from '../../components/SectionCard';
import type { TagInventoryEntry } from '../../types/aria';

type TagInventoryPanelProps = {
  inventory: TagInventoryEntry[];
};

export function TagInventoryPanel({ inventory }: TagInventoryPanelProps) {
  return (
    <SectionCard eyebrow="Tags" title="Observed tags in scanned files">
      <ul className="inventory-list">
        {inventory.length === 0 ? (
          <li className="empty-state">Scan a library to build the tag list.</li>
        ) : (
          inventory.map((entry) => (
            <li key={entry.tag}>
              <div>
                <strong>{entry.tag}</strong>
                <span>{entry.exampleValues.join(' • ') || 'No example value'}</span>
              </div>
              <code>{entry.occurrences}</code>
            </li>
          ))
        )}
      </ul>
    </SectionCard>
  );
}
