import { useEffect, useRef, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type {
  DiagnosticLogEntry,
  DiagnosticLogLevel,
  DiagnosticSubsystem,
  LoggingSettings,
} from '../../types/aria';
import {
  clearDiagnosticLogs,
  copyToClipboard,
  exportDiagnosticLogsText,
  getDiagnosticLogPath,
  getDiagnosticLogs,
  updateLoggingSettings,
} from '../../lib/aria';

type DiagnosticLogsPanelProps = {
  loggingSettings?: LoggingSettings;
  onLoggingSettingsChange?: (settings: LoggingSettings) => void;
};

const DEFAULT_LOGGING_SETTINGS: LoggingSettings = {
  enabled: true,
  minLevel: 'debug',
  logToFile: true,
  logDownloading: true,
  logPlayback: true,
  maxEntries: 1000,
};

export function DiagnosticLogsPanel({
  loggingSettings,
  onLoggingSettingsChange,
}: DiagnosticLogsPanelProps) {
  const settings = loggingSettings ?? DEFAULT_LOGGING_SETTINGS;
  const [logs, setLogs] = useState<DiagnosticLogEntry[]>([]);
  const [logFilePath, setLogFilePath] = useState<string | null>(null);
  const [selectedSubsystem, setSelectedSubsystem] = useState<string>('all');
  const [filterLevel, setFilterLevel] = useState<string>('all');
  const [searchText, setSearchText] = useState('');
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [copied, setCopied] = useState(false);
  const [loading, setLoading] = useState(false);
  const logContainerRef = useRef<HTMLDivElement>(null);

  const fetchLogs = async () => {
    try {
      const sub = selectedSubsystem === 'all' ? undefined : selectedSubsystem;
      const minLvl =
        filterLevel === 'all' ? undefined : (filterLevel as DiagnosticLogLevel);
      const entries = await getDiagnosticLogs(sub, minLvl, 500);
      setLogs(entries);
    } catch (err) {
      console.warn('Failed to fetch diagnostic logs:', err);
    }
  };

  useEffect(() => {
    getDiagnosticLogPath().then(setLogFilePath).catch(console.warn);
    fetchLogs();
  }, [selectedSubsystem, filterLevel]);

  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(() => {
      fetchLogs();
    }, 1500);
    return () => clearInterval(interval);
  }, [autoRefresh, selectedSubsystem, filterLevel]);

  const handleToggleEnabled = (enabled: boolean) => {
    const updated = { ...settings, enabled };
    onLoggingSettingsChange?.(updated);
    updateLoggingSettings(updated).catch(console.error);
  };

  const handleMinLevelChange = (minLevel: DiagnosticLogLevel) => {
    const updated = { ...settings, minLevel };
    onLoggingSettingsChange?.(updated);
    updateLoggingSettings(updated).catch(console.error);
  };

  const handleToggleLogToFile = (logToFile: boolean) => {
    const updated = { ...settings, logToFile };
    onLoggingSettingsChange?.(updated);
    updateLoggingSettings(updated).catch(console.error);
  };

  const handleToggleDownloading = (logDownloading: boolean) => {
    const updated = { ...settings, logDownloading };
    onLoggingSettingsChange?.(updated);
    updateLoggingSettings(updated).catch(console.error);
  };

  const handleTogglePlayback = (logPlayback: boolean) => {
    const updated = { ...settings, logPlayback };
    onLoggingSettingsChange?.(updated);
    updateLoggingSettings(updated).catch(console.error);
  };

  const handleClear = async () => {
    try {
      await clearDiagnosticLogs();
      setLogs([]);
    } catch (err) {
      console.error('Failed to clear logs:', err);
    }
  };

  const handleCopy = async () => {
    try {
      const text = await exportDiagnosticLogsText();
      await copyToClipboard(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to export/copy logs:', err);
    }
  };

  const formatTimestamp = (timestampMs: number) => {
    const d = new Date(timestampMs);
    const h = String(d.getHours()).padStart(2, '0');
    const m = String(d.getMinutes()).padStart(2, '0');
    const s = String(d.getSeconds()).padStart(2, '0');
    const ms = String(d.getMilliseconds()).padStart(3, '0');
    return `${h}:${m}:${s}.${ms}`;
  };

  const levelColor = (level: DiagnosticLogLevel) => {
    switch (level) {
      case 'error':
        return '#f87171'; // red
      case 'warn':
        return '#fbbf24'; // amber/yellow
      case 'info':
        return '#60a5fa'; // blue
      case 'debug':
        return '#a78bfa'; // purple
      case 'trace':
        return '#9ca3af'; // gray
      default:
        return '#e5e7eb';
    }
  };

  const subsystemBadge = (subsystem: DiagnosticSubsystem) => {
    switch (subsystem) {
      case 'playback':
        return { label: 'Playback', bg: 'rgba(59, 130, 246, 0.15)', text: '#93c5fd' };
      case 'download':
        return { label: 'Download', bg: 'rgba(16, 185, 129, 0.15)', text: '#6ee7b7' };
      case 'storage':
        return { label: 'Storage', bg: 'rgba(245, 158, 11, 0.15)', text: '#fcd34d' };
      default:
        return { label: 'General', bg: 'rgba(107, 114, 128, 0.15)', text: '#d1d5db' };
    }
  };

  const filteredLogs = logs.filter((log) => {
    if (!searchText) return true;
    const q = searchText.toLowerCase();
    return (
      log.message.toLowerCase().includes(q) ||
      (log.details && log.details.toLowerCase().includes(q)) ||
      log.subsystem.toLowerCase().includes(q)
    );
  });

  return (
    <SectionCard eyebrow="Diagnostics" title="Downloading & playback logs">
      <p className="dialog-section__note" style={{ marginBottom: 12 }}>
        Investigate streaming throughput, prefetching progress, cache hits/misses,
        and audio pipeline buffer underruns in real time on this device.
      </p>

      {/* Configuration Controls */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(220px, 1fr))',
          gap: 12,
          padding: '12px 14px',
          background: 'rgba(255, 255, 255, 0.03)',
          borderRadius: 8,
          border: '1px solid rgba(255, 255, 255, 0.07)',
          marginBottom: 16,
        }}
      >
        <label className="checkbox-row" style={{ margin: 0 }}>
          <input
            checked={settings.enabled}
            onChange={(e) => handleToggleEnabled(e.target.checked)}
            type="checkbox"
          />
          <span>Enable diagnostic logging</span>
        </label>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ fontSize: '0.85rem', color: 'var(--color-text-secondary, #9ca3af)', whiteSpace: 'nowrap' }}>
            Min Level:
          </span>
          <select
            value={settings.minLevel}
            onChange={(e) => handleMinLevelChange(e.target.value as DiagnosticLogLevel)}
            style={{
              padding: '4px 8px',
              fontSize: '0.82rem',
              borderRadius: 4,
              background: 'var(--color-bg-secondary, #1f2937)',
              color: 'inherit',
              border: '1px solid rgba(255, 255, 255, 0.1)',
            }}
          >
            <option value="trace">Trace (all)</option>
            <option value="debug">Debug</option>
            <option value="info">Info</option>
            <option value="warn">Warn</option>
            <option value="error">Error only</option>
          </select>
        </div>

        <label className="checkbox-row" style={{ margin: 0 }}>
          <input
            checked={settings.logToFile}
            onChange={(e) => handleToggleLogToFile(e.target.checked)}
            type="checkbox"
          />
          <span>Save logs to local disk file</span>
        </label>

        <label className="checkbox-row" style={{ margin: 0 }}>
          <input
            checked={settings.logDownloading}
            onChange={(e) => handleToggleDownloading(e.target.checked)}
            type="checkbox"
          />
          <span>Log downloading & prefetch</span>
        </label>

        <label className="checkbox-row" style={{ margin: 0 }}>
          <input
            checked={settings.logPlayback}
            onChange={(e) => handleTogglePlayback(e.target.checked)}
            type="checkbox"
          />
          <span>Log AudioTrack & playback</span>
        </label>
      </div>

      {logFilePath && (
        <div
          style={{
            fontSize: '0.78rem',
            color: 'var(--color-text-muted, #9ca3af)',
            marginBottom: 12,
            overflowWrap: 'break-word',
          }}
        >
          <strong>Log file:</strong> <span style={{ fontFamily: 'monospace' }}>{logFilePath}</span>
        </div>
      )}

      {/* Filter and Action Bar */}
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 8,
          marginBottom: 10,
        }}
      >
        <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: 6 }}>
          {['all', 'playback', 'download', 'storage', 'general'].map((sub) => (
            <button
              key={sub}
              onClick={() => setSelectedSubsystem(sub)}
              style={{
                padding: '3px 9px',
                fontSize: '0.78rem',
                borderRadius: 4,
                border: '1px solid',
                borderColor: selectedSubsystem === sub ? 'var(--color-accent, #d6b16a)' : 'rgba(255, 255, 255, 0.1)',
                background: selectedSubsystem === sub ? 'rgba(214, 177, 106, 0.15)' : 'transparent',
                color: selectedSubsystem === sub ? 'var(--color-accent, #d6b16a)' : 'inherit',
                cursor: 'pointer',
                textTransform: 'capitalize',
              }}
            >
              {sub}
            </button>
          ))}
        </div>

        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <input
            placeholder="Search logs..."
            value={searchText}
            onChange={(e) => setSearchText(e.target.value)}
            style={{
              padding: '4px 8px',
              fontSize: '0.78rem',
              borderRadius: 4,
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: 'rgba(0, 0, 0, 0.2)',
              color: 'inherit',
              width: 140,
            }}
          />

          <button
            onClick={() => setAutoRefresh(!autoRefresh)}
            title="Toggle live auto-refresh"
            style={{
              padding: '4px 8px',
              fontSize: '0.78rem',
              borderRadius: 4,
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: autoRefresh ? 'rgba(34, 197, 94, 0.2)' : 'transparent',
              color: autoRefresh ? '#4ade80' : 'inherit',
              cursor: 'pointer',
            }}
          >
            {autoRefresh ? 'Live' : 'Paused'}
          </button>

          <button
            onClick={fetchLogs}
            style={{
              padding: '4px 8px',
              fontSize: '0.78rem',
              borderRadius: 4,
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: 'transparent',
              cursor: 'pointer',
            }}
          >
            Refresh
          </button>

          <button
            onClick={handleCopy}
            style={{
              padding: '4px 8px',
              fontSize: '0.78rem',
              borderRadius: 4,
              border: '1px solid rgba(255, 255, 255, 0.1)',
              background: 'transparent',
              cursor: 'pointer',
            }}
          >
            {copied ? 'Copied!' : 'Copy Logs'}
          </button>

          <button
            onClick={handleClear}
            style={{
              padding: '4px 8px',
              fontSize: '0.78rem',
              borderRadius: 4,
              border: '1px solid rgba(239, 68, 68, 0.3)',
              background: 'rgba(239, 68, 68, 0.1)',
              color: '#f87171',
              cursor: 'pointer',
            }}
          >
            Clear
          </button>
        </div>
      </div>

      {/* Log Entries Terminal Window */}
      <div
        ref={logContainerRef}
        style={{
          height: 340,
          overflowY: 'auto',
          background: '#0a0d14',
          borderRadius: 6,
          border: '1px solid rgba(255, 255, 255, 0.08)',
          padding: 8,
          fontFamily: 'monospace, Consolas, "Liberation Mono", Menlo, Courier',
          fontSize: '0.74rem',
          lineHeight: '1.45',
        }}
      >
        {filteredLogs.length === 0 ? (
          <div style={{ color: '#6b7280', padding: 16, textAlign: 'center' }}>
            No diagnostic events recorded yet. Play a track or start a download to see live diagnostic logs.
          </div>
        ) : (
          filteredLogs.map((entry) => {
            const badge = subsystemBadge(entry.subsystem);
            return (
              <div
                key={entry.id}
                style={{
                  display: 'flex',
                  alignItems: 'baseline',
                  gap: 6,
                  padding: '2px 0',
                  borderBottom: '1px solid rgba(255, 255, 255, 0.02)',
                }}
              >
                <span style={{ color: '#6b7280', whiteSpace: 'nowrap', flexShrink: 0 }}>
                  {formatTimestamp(entry.timestampMs)}
                </span>

                <span
                  style={{
                    color: levelColor(entry.level),
                    fontWeight: 600,
                    width: 44,
                    flexShrink: 0,
                    textTransform: 'uppercase',
                  }}
                >
                  {entry.level}
                </span>

                <span
                  style={{
                    background: badge.bg,
                    color: badge.text,
                    padding: '1px 5px',
                    borderRadius: 3,
                    fontSize: '0.7rem',
                    flexShrink: 0,
                  }}
                >
                  {badge.label}
                </span>

                <span style={{ color: '#e5e7eb', wordBreak: 'break-word', flex: 1 }}>
                  {entry.message}
                  {entry.details && (
                    <span style={{ color: '#9ca3af', marginLeft: 6 }}>
                      ({entry.details})
                    </span>
                  )}
                </span>
              </div>
            );
          })
        )}
      </div>

      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginTop: 6,
          fontSize: '0.72rem',
          color: '#6b7280',
        }}
      >
        <span>
          Showing {filteredLogs.length} of {logs.length} events
        </span>
        <span>
          Includes JNI AudioTrack hardware metrics, WiFi locks, and network transfer speeds
        </span>
      </div>
    </SectionCard>
  );
}
