import { useEffect, useState, type ReactNode } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type {
  CatalogPatternRule,
  LibraryFieldMapping,
  LibrarySnapshot,
  OutputDeviceSnapshot,
  PlaybackPreferences,
  SettingsSnapshot,
  ThemePreference,
} from '../../types/aria';
import { CatalogRulesPanel } from '../library/CatalogRulesPanel';
import { FieldMappingsPanel } from '../library/FieldMappingsPanel';
import { LibraryPanel } from '../library/LibraryPanel';
import { PlaybackSettingsPanel } from './PlaybackSettingsPanel';
import { SettingsPanel } from './SettingsPanel';

type SettingsPaneProps = {
  library: LibrarySnapshot;
  currentOutputDevice: OutputDeviceSnapshot;
  outputDevices: OutputDeviceSnapshot[];
  draftMappings: LibraryFieldMapping[];
  draftCatalogRules: CatalogPatternRule[];
  settings: SettingsSnapshot;
  onAddDirectory: () => void;
  onClearLibrary: () => void;
  onRemoveRoot: (path: string) => void;
  onRescanAll: () => void;
  onAddField: () => void;
  onAddCatalogRule: () => void;
  onRemoveField: (index: number) => void;
  onRemoveCatalogRule: (index: number) => void;
  onUpdateField: (index: number, patch: Partial<LibraryFieldMapping>) => void;
  onUpdateCatalogRule: (index: number, patch: Partial<CatalogPatternRule>) => void;
  onSaveMappings: () => void;
  onSaveCatalogRules: () => void;
  onThemeChange: (theme: ThemePreference) => void;
  onPlaybackPreferencesChange: (playback: PlaybackPreferences) => void;
};

export function SettingsPane({
  library,
  currentOutputDevice,
  outputDevices,
  draftMappings,
  draftCatalogRules,
  settings,
  onAddDirectory,
  onClearLibrary,
  onRemoveRoot,
  onRescanAll,
  onAddField,
  onAddCatalogRule,
  onRemoveField,
  onRemoveCatalogRule,
  onUpdateField,
  onUpdateCatalogRule,
  onSaveMappings,
  onSaveCatalogRules,
  onThemeChange,
  onPlaybackPreferencesChange,
}: SettingsPaneProps) {
  const [isMappingsDialogOpen, setIsMappingsDialogOpen] = useState(false);
  const [isCatalogRulesDialogOpen, setIsCatalogRulesDialogOpen] = useState(false);

  useEffect(() => {
    if (!isMappingsDialogOpen && !isCatalogRulesDialogOpen) {
      return undefined;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setIsMappingsDialogOpen(false);
        setIsCatalogRulesDialogOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isCatalogRulesDialogOpen, isMappingsDialogOpen]);

  return (
    <div className="pane-stack">
      <div className="settings-grid">
        <LibraryPanel
          library={library}
          onAddDirectory={onAddDirectory}
          onClearLibrary={onClearLibrary}
          onRemoveRoot={onRemoveRoot}
          onRescanAll={onRescanAll}
        />
        <div className="pane-stack">
          <SectionCard
            eyebrow="Mappings"
            title="Database fields"
            actions={
              <button
                className="ghost-button"
                onClick={() => setIsMappingsDialogOpen(true)}
                type="button"
              >
                Edit mappings
              </button>
            }
          >
            <p className="panel-copy">
              Choose which normalized fields Aria stores and which source tags
              fill them in priority order.
            </p>
            <div className="device-chip">
              <strong>{draftMappings.length} fields configured</strong>
              <span>Empty fields are allowed and multi-value tags are preserved.</span>
            </div>
          </SectionCard>

          <SectionCard
            eyebrow="Catalog Rules"
            title="Catalog extraction"
            actions={
              <button
                className="ghost-button"
                onClick={() => setIsCatalogRulesDialogOpen(true)}
                type="button"
              >
                Edit rules
              </button>
            }
          >
            <p className="panel-copy">
              Add composer-aware regex patterns for schemes like BWV, WAB, K.,
              or your own catalog abbreviations.
            </p>
            <div className="device-chip">
              <strong>{draftCatalogRules.length} rules configured</strong>
              <span>Rules can be enabled, ordered by source tags, and saved independently.</span>
            </div>
          </SectionCard>
        </div>
      </div>

      <div className="settings-grid settings-grid--secondary">
        <PlaybackSettingsPanel
          currentOutputDevice={currentOutputDevice}
          onChange={onPlaybackPreferencesChange}
          outputDevices={outputDevices}
          playback={settings.playback}
        />

        <SettingsPanel
          onThemeChange={onThemeChange}
          settings={settings}
        />
      </div>

      {isMappingsDialogOpen ? (
        <ConfigDialog
          copy="Adjust the database fields Aria stores and define which tags populate each field in descending priority."
          eyebrow="Mappings"
          onClose={() => setIsMappingsDialogOpen(false)}
          title="Database field mapping"
        >
          <FieldMappingsPanel
            mappings={draftMappings}
            onAddField={onAddField}
            onRemoveField={onRemoveField}
            onSave={onSaveMappings}
            onUpdateField={onUpdateField}
            variant="dialog"
          />
        </ConfigDialog>
      ) : null}

      {isCatalogRulesDialogOpen ? (
        <ConfigDialog
          copy="Add built-in or custom composer-aware catalog matchers. Rules are used when the catalog field is still empty after tag mapping."
          eyebrow="Catalog Rules"
          onClose={() => setIsCatalogRulesDialogOpen(false)}
          title="Catalog extraction rules"
        >
          <CatalogRulesPanel
            rules={draftCatalogRules}
            onAddRule={onAddCatalogRule}
            onRemoveRule={onRemoveCatalogRule}
            onSave={onSaveCatalogRules}
            onUpdateRule={onUpdateCatalogRule}
            variant="dialog"
          />
        </ConfigDialog>
      ) : null}
    </div>
  );
}

type ConfigDialogProps = {
  children: ReactNode;
  copy: string;
  eyebrow: string;
  onClose: () => void;
  title: string;
};

function ConfigDialog({
  children,
  copy,
  eyebrow,
  onClose,
  title,
}: ConfigDialogProps) {
  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="settings-config-dialog-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">{eyebrow}</p>
            <h3 id="settings-config-dialog-title">{title}</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">{copy}</p>
        {children}
      </div>
    </div>
  );
}
