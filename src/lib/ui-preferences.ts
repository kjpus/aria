export type AccentColorKey =
  | 'gold'
  | 'blue'
  | 'emerald'
  | 'crimson'
  | 'violet'
  | 'titanium';

export type UiDensity = 'compact' | 'comfortable';
export type NavStyle = 'sidebar' | 'topbar';
export type AlbumGridSize = 'small' | 'medium' | 'large';
export type FontScale = 'compact' | 'normal' | 'large';

export interface AccentColorPreset {
  key: AccentColorKey;
  label: string;
  accent: string;
  accentStrong: string;
  accentSoft: string;
  accentGlow: string;
}

export const ACCENT_PRESETS: Record<AccentColorKey, AccentColorPreset> = {
  gold: {
    key: 'gold',
    label: 'Champagne Gold',
    accent: '#d4af65',
    accentStrong: '#e8c988',
    accentSoft: 'rgba(212, 175, 101, 0.16)',
    accentGlow: 'rgba(212, 175, 101, 0.35)',
  },
  blue: {
    key: 'blue',
    label: 'Nordic Slate',
    accent: '#4f9ed8',
    accentStrong: '#76b8ec',
    accentSoft: 'rgba(79, 158, 216, 0.16)',
    accentGlow: 'rgba(79, 158, 216, 0.35)',
  },
  emerald: {
    key: 'emerald',
    label: 'Concert Emerald',
    accent: '#38b2ac',
    accentStrong: '#4fd1c5',
    accentSoft: 'rgba(56, 178, 172, 0.16)',
    accentGlow: 'rgba(56, 178, 172, 0.35)',
  },
  crimson: {
    key: 'crimson',
    label: 'Opera Crimson',
    accent: '#e55353',
    accentStrong: '#f57575',
    accentSoft: 'rgba(229, 83, 83, 0.16)',
    accentGlow: 'rgba(229, 83, 83, 0.35)',
  },
  violet: {
    key: 'violet',
    label: 'Amethyst Royale',
    accent: '#9f7aea',
    accentStrong: '#b794f4',
    accentSoft: 'rgba(159, 122, 234, 0.16)',
    accentGlow: 'rgba(159, 122, 234, 0.35)',
  },
  titanium: {
    key: 'titanium',
    label: 'Studio Titanium',
    accent: '#a0aec0',
    accentStrong: '#cbd5e0',
    accentSoft: 'rgba(160, 174, 192, 0.16)',
    accentGlow: 'rgba(160, 174, 192, 0.35)',
  },
};

export interface UiPreferences {
  accent: AccentColorKey;
  density: UiDensity;
  navStyle: NavStyle;
  gridSize: AlbumGridSize;
  fontScale: FontScale;
  sidebarCollapsed: boolean;
}

export const DEFAULT_UI_PREFERENCES: UiPreferences = {
  accent: 'gold',
  density: 'comfortable',
  navStyle: 'sidebar',
  gridSize: 'medium',
  fontScale: 'normal',
  sidebarCollapsed: false,
};

const STORAGE_KEY = 'aria_ui_preferences_v2';

export function loadUiPreferences(): UiPreferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_UI_PREFERENCES;
    }
    const parsed = JSON.parse(raw) as Partial<UiPreferences>;
    return {
      accent: parsed.accent && ACCENT_PRESETS[parsed.accent] ? parsed.accent : DEFAULT_UI_PREFERENCES.accent,
      density: parsed.density === 'compact' ? 'compact' : 'comfortable',
      navStyle: parsed.navStyle === 'topbar' ? 'topbar' : 'sidebar',
      gridSize: parsed.gridSize === 'small' || parsed.gridSize === 'large' ? parsed.gridSize : 'medium',
      fontScale: parsed.fontScale === 'compact' || parsed.fontScale === 'large' ? parsed.fontScale : 'normal',
      sidebarCollapsed: Boolean(parsed.sidebarCollapsed),
    };
  } catch {
    return DEFAULT_UI_PREFERENCES;
  }
}

export function saveUiPreferences(prefs: UiPreferences): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Ignore quota or private mode errors
  }
}

export function applyUiPreferences(prefs: UiPreferences): void {
  const root = document.documentElement;
  const preset = ACCENT_PRESETS[prefs.accent] ?? ACCENT_PRESETS.gold;

  root.style.setProperty('--accent', preset.accent);
  root.style.setProperty('--accent-strong', preset.accentStrong);
  root.style.setProperty('--accent-soft', preset.accentSoft);
  root.style.setProperty('--accent-glow', preset.accentGlow);

  root.dataset.density = prefs.density;
  root.dataset.navStyle = prefs.navStyle;
  root.dataset.fontScale = prefs.fontScale;
  root.dataset.gridSize = prefs.gridSize;
}
