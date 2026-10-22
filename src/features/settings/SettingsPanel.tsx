import { SectionCard } from '../../components/SectionCard';
import { CheckIcon } from '../../components/Icons';
import type { SettingsSnapshot, ThemePreference } from '../../types/aria';
import {
  ACCENT_PRESETS,
  type AccentColorKey,
  type AlbumGridSize,
  type FontScale,
  type NavStyle,
  type UiDensity,
  type UiPreferences,
} from '../../lib/ui-preferences';

type SettingsPanelProps = {
  settings: SettingsSnapshot;
  onThemeChange: (theme: ThemePreference) => void;
  uiPrefs: UiPreferences;
  onUiPrefsChange: (prefs: UiPreferences) => void;
};

const themeOptions: { key: ThemePreference; label: string }[] = [
  { key: 'system', label: 'System' },
  { key: 'dark', label: 'Dark' },
  { key: 'light', label: 'Light' },
];

const densityOptions: { key: UiDensity; label: string; desc: string }[] = [
  { key: 'comfortable', label: 'Comfortable', desc: 'Balanced spacing, ideal for relaxed browsing' },
  { key: 'compact', label: 'Compact', desc: 'Tight row heights, fits 40% more classical works' },
];

const navOptions: { key: NavStyle; label: string; desc: string }[] = [
  { key: 'sidebar', label: 'Desktop Sidebar', desc: 'Native side navigation with pane icons and badge counters' },
  { key: 'topbar', label: 'Top Bar', desc: 'Horizontal segmented bar, maximizing table column width' },
];

const gridOptions: { key: AlbumGridSize; label: string }[] = [
  { key: 'small', label: 'Compact (160px)' },
  { key: 'medium', label: 'Standard (215px)' },
  { key: 'large', label: 'Spacious (280px)' },
];

const fontScaleOptions: { key: FontScale; label: string }[] = [
  { key: 'compact', label: 'Compact (92%)' },
  { key: 'normal', label: 'Default (100%)' },
  { key: 'large', label: 'Large (108%)' },
];

export function SettingsPanel({
  settings,
  onThemeChange,
  uiPrefs,
  onUiPrefsChange,
}: SettingsPanelProps) {
  function handleAccentSelect(key: AccentColorKey) {
    onUiPrefsChange({
      ...uiPrefs,
      accent: key,
    });
  }

  function handleDensitySelect(density: UiDensity) {
    onUiPrefsChange({
      ...uiPrefs,
      density,
    });
  }

  function handleNavStyleSelect(navStyle: NavStyle) {
    onUiPrefsChange({
      ...uiPrefs,
      navStyle,
    });
  }

  function handleGridSizeSelect(gridSize: AlbumGridSize) {
    onUiPrefsChange({
      ...uiPrefs,
      gridSize,
    });
  }

  function handleFontScaleSelect(fontScale: FontScale) {
    onUiPrefsChange({
      ...uiPrefs,
      fontScale,
    });
  }

  return (
    <SectionCard eyebrow="Appearance" title="Interface & Personalization">
      <div className="field-stack settings-section-group">
        {/* Theme mode */}
        <div className="settings-field">
          <label className="field-label">Color Scheme</label>
          <div aria-label="Theme mode" className="segmented-control" role="group">
            {themeOptions.map((opt) => (
              <button
                aria-pressed={settings.theme === opt.key}
                className={settings.theme === opt.key ? 'segmented-control__active' : ''}
                key={opt.key}
                onClick={() => onThemeChange(opt.key)}
                type="button"
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>

        {/* Accent Color Palette */}
        <div className="settings-field">
          <div className="settings-field__header">
            <label className="field-label">Accent Palette</label>
            <span className="settings-field__hint">
              {ACCENT_PRESETS[uiPrefs.accent]?.label ?? 'Custom'}
            </span>
          </div>
          <div className="color-swatch-grid" role="radiogroup">
            {(Object.keys(ACCENT_PRESETS) as AccentColorKey[]).map((key) => {
              const preset = ACCENT_PRESETS[key];
              const isSelected = uiPrefs.accent === key;
              return (
                <button
                  aria-checked={isSelected}
                  aria-label={preset.label}
                  className={`color-swatch-button${isSelected ? ' color-swatch-button--active' : ''}`}
                  key={key}
                  onClick={() => handleAccentSelect(key)}
                  role="radio"
                  style={{ '--swatch-color': preset.accent } as React.CSSProperties}
                  title={preset.label}
                  type="button"
                >
                  <span className="color-swatch-dot" style={{ backgroundColor: preset.accent }} />
                  {isSelected ? <CheckIcon className="color-swatch-check" size={12} /> : null}
                </button>
              );
            })}
          </div>
        </div>

        {/* Navigation Style */}
        <div className="settings-field">
          <label className="field-label">Navigation Style</label>
          <div className="segmented-cards-group">
            {navOptions.map((opt) => (
              <button
                aria-pressed={uiPrefs.navStyle === opt.key}
                className={`segmented-card-button${uiPrefs.navStyle === opt.key ? ' segmented-card-button--active' : ''}`}
                key={opt.key}
                onClick={() => handleNavStyleSelect(opt.key)}
                type="button"
              >
                <strong>{opt.label}</strong>
                <span>{opt.desc}</span>
              </button>
            ))}
          </div>
        </div>

        {/* UI Density */}
        <div className="settings-field">
          <label className="field-label">Information Density</label>
          <div className="segmented-cards-group">
            {densityOptions.map((opt) => (
              <button
                aria-pressed={uiPrefs.density === opt.key}
                className={`segmented-card-button${uiPrefs.density === opt.key ? ' segmented-card-button--active' : ''}`}
                key={opt.key}
                onClick={() => handleDensitySelect(opt.key)}
                type="button"
              >
                <strong>{opt.label}</strong>
                <span>{opt.desc}</span>
              </button>
            ))}
          </div>
        </div>

        {/* Album Grid Cover Size */}
        <div className="settings-field">
          <label className="field-label">Album Cover Size</label>
          <div aria-label="Album cover size" className="segmented-control" role="group">
            {gridOptions.map((opt) => (
              <button
                aria-pressed={uiPrefs.gridSize === opt.key}
                className={uiPrefs.gridSize === opt.key ? 'segmented-control__active' : ''}
                key={opt.key}
                onClick={() => handleGridSizeSelect(opt.key)}
                type="button"
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>

        {/* Font Scale */}
        <div className="settings-field">
          <label className="field-label">UI Font Scale</label>
          <div aria-label="Font scale" className="segmented-control" role="group">
            {fontScaleOptions.map((opt) => (
              <button
                aria-pressed={uiPrefs.fontScale === opt.key}
                className={uiPrefs.fontScale === opt.key ? 'segmented-control__active' : ''}
                key={opt.key}
                onClick={() => handleFontScaleSelect(opt.key)}
                type="button"
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>
      </div>
    </SectionCard>
  );
}
