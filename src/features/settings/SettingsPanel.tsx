import { SectionCard } from '../../components/SectionCard';
import type { SettingsSnapshot, ThemePreference } from '../../types/aria';

type SettingsPanelProps = {
  settings: SettingsSnapshot;
  onThemeChange: (theme: ThemePreference) => void;
};

const themeOptions: ThemePreference[] = ['system', 'light', 'dark'];

export function SettingsPanel({
  settings,
  onThemeChange,
}: SettingsPanelProps) {
  return (
    <SectionCard eyebrow="Settings" title="Prefererence">
      <div className="field-stack">
        <label className="field-label" htmlFor="theme-select">
          Theme
        </label>
        <select
          id="theme-select"
          value={settings.theme}
          onChange={(event) =>
            onThemeChange(event.target.value as ThemePreference)
          }
        >
          {themeOptions.map((theme) => (
            <option key={theme} value={theme}>
              {theme}
            </option>
          ))}
        </select>
      </div>
    </SectionCard>
  );
}
