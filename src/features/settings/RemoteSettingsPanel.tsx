import { useEffect, useState } from 'react';
import { SectionCard } from '../../components/SectionCard';
import type { RemoteBackendType, RemoteCacheStatus, RemoteTarget, StorageStatus } from '../../types/aria';
import {
  clearRemoteCache,
  deleteRemoteTarget,
  getRemoteCacheStatus,
  saveRemoteTarget,
  startGdriveAuthFlow,
  completeGdriveAuthFlow,
  testRemoteTarget,
} from '../../lib/aria';

type RemoteSettingsPanelProps = {
  targets: RemoteTarget[];
  onTargetsChange: (targets: RemoteTarget[]) => void;
};

export function RemoteSettingsPanel({
  targets,
  onTargetsChange,
}: RemoteSettingsPanelProps) {
  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  const [testingTargetId, setTestingTargetId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, StorageStatus>>({});
  const [actionError, setActionError] = useState<string | null>(null);
  const [cacheStatus, setCacheStatus] = useState<RemoteCacheStatus | null>(null);
  const [isClearing, setIsClearing] = useState(false);

  useEffect(() => {
    getRemoteCacheStatus().then(setCacheStatus).catch(console.error);
  }, []);

  async function handleClearCache() {
    if (!confirm('Are you sure you want to clear all cached audio files from disk?')) {
      return;
    }
    setIsClearing(true);
    setActionError(null);
    try {
      await clearRemoteCache();
      const updated = await getRemoteCacheStatus();
      setCacheStatus(updated);
    } catch (err) {
      setActionError(`Failed to clear cache: ${String(err)}`);
    } finally {
      setIsClearing(false);
    }
  }

  async function handleTest(targetId: string) {
    setTestingTargetId(targetId);
    setActionError(null);
    try {
      const status = await testRemoteTarget(targetId);
      setTestResults((prev) => ({ ...prev, [targetId]: status }));
    } catch (err) {
      setActionError(`Test failed: ${String(err)}`);
    } finally {
      setTestingTargetId(null);
    }
  }

  async function handleDelete(targetId: string) {
    if (!confirm('Are you sure you want to remove this remote storage target?')) {
      return;
    }
    setActionError(null);
    try {
      const updated = await deleteRemoteTarget(targetId);
      onTargetsChange(updated);
    } catch (err) {
      setActionError(`Failed to delete target: ${String(err)}`);
    }
  }

  async function handleToggle(target: RemoteTarget) {
    try {
      const updated = await saveRemoteTarget({
        ...target,
        isEnabled: !target.isEnabled,
      });
      onTargetsChange(updated);
    } catch (err) {
      setActionError(`Failed to update target: ${String(err)}`);
    }
  }

  return (
    <div style={{ display: 'grid', gap: '1.25rem' }}>
      <SectionCard
        eyebrow="Cloud & Remote"
        title="Remote storage targets"
        actions={
          <button
            className="ghost-button"
            onClick={() => setIsAddDialogOpen(true)}
            type="button"
          >
            Add remote
          </button>
        }
      >
        <p className="panel-copy">
          Configure remote storage targets (Google Drive, SMB network share, WebDAV, or local/mounted filesystem)
          to upload albums with pre-extracted classical manifests and library master indexes.
        </p>

        {actionError ? (
          <div className="error-banner" style={{ marginBottom: '1rem' }}>
            {actionError}
          </div>
        ) : null}

        <div className="metrics-grid">
          <div>
            <span>Configured Remotes</span>
            <strong>{targets.length}</strong>
          </div>
          <div>
            <span>Enabled Remotes</span>
            <strong>{targets.filter((t) => t.isEnabled).length}</strong>
          </div>
        </div>

        <div style={{ display: 'grid', gap: '0.8rem', marginTop: '1rem' }}>
          {targets.length === 0 ? (
            <p className="panel-copy" style={{ fontStyle: 'italic' }}>
              No remote storage targets configured yet. Click &ldquo;Add remote&rdquo; to connect Google Drive, SMB share, WebDAV, or filesystem path.
            </p>
          ) : (
            targets.map((target) => {
              const status = testResults[target.id];
              const isTesting = testingTargetId === target.id;

              return (
                <div
                  key={target.id}
                  style={{
                    padding: '0.8rem 1rem',
                    borderRadius: '8px',
                    border: '1px solid var(--border-subtle)',
                    background: 'var(--surface-sunken)',
                  }}
                >
                  <div
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      marginBottom: '0.4rem',
                    }}
                  >
                    <div>
                      <strong style={{ fontSize: '0.95rem' }}>{target.name}</strong>
                      <span
                        style={{
                          marginLeft: '0.6rem',
                          fontSize: '0.75rem',
                          padding: '2px 8px',
                          borderRadius: '12px',
                          background: 'var(--surface-elevated)',
                          color: 'var(--muted)',
                        }}
                      >
                        {formatBackendLabel(target.backendType)}
                      </span>
                    </div>

                    <div style={{ display: 'flex', gap: '0.4rem' }}>
                      <button
                        className="ghost-button"
                        disabled={isTesting}
                        onClick={() => handleTest(target.id)}
                        style={{ fontSize: '0.8rem', padding: '2px 8px' }}
                        type="button"
                      >
                        {isTesting ? 'Testing...' : 'Test'}
                      </button>
                      <button
                        className="ghost-button"
                        onClick={() => handleToggle(target)}
                        style={{ fontSize: '0.8rem', padding: '2px 8px' }}
                        type="button"
                      >
                        {target.isEnabled ? 'Disable' : 'Enable'}
                      </button>
                      <button
                        className="ghost-button"
                        onClick={() => handleDelete(target.id)}
                        style={{ fontSize: '0.8rem', padding: '2px 8px', color: '#e57373' }}
                        type="button"
                      >
                        Delete
                      </button>
                    </div>
                  </div>

                  <div style={{ fontSize: '0.8rem', color: 'var(--muted)' }}>
                    Quota: {target.storageLimitBytes ? formatBytes(target.storageLimitBytes) : 'Unlimited'}
                    {' • '}
                    Status: {target.isEnabled ? 'Active' : 'Disabled'}
                  </div>

                  {status ? (
                    <div
                      style={{
                        marginTop: '0.5rem',
                        fontSize: '0.8rem',
                        padding: '0.4rem 0.6rem',
                        borderRadius: '4px',
                        background: status.isConnected ? 'rgba(76, 175, 80, 0.1)' : 'rgba(244, 67, 54, 0.1)',
                        color: status.isConnected ? '#81c784' : '#e57373',
                      }}
                    >
                      {status.isConnected ? '✓ Connected' : '✗ Connection Failed'}: {status.message}
                      {status.storageUsage ? (
                        <div style={{ marginTop: '0.2rem', color: 'var(--muted)' }}>
                          Used: {formatBytes(status.storageUsage.usedBytes)}
                          {status.storageUsage.totalBytes ? ` / ${formatBytes(status.storageUsage.totalBytes)}` : ''}
                          {status.storageUsage.freeBytes ? ` (${formatBytes(status.storageUsage.freeBytes)} free)` : ''}
                        </div>
                      ) : null}
                    </div>
                  ) : null}
                </div>
              );
            })
          )}
        </div>

        {isAddDialogOpen ? (
          <AddRemoteDialog
            onClose={() => setIsAddDialogOpen(false)}
            onAdded={(newTargets) => {
              onTargetsChange(newTargets);
              setIsAddDialogOpen(false);
            }}
          />
        ) : null}
      </SectionCard>

      <SectionCard
        eyebrow="Streaming & Cache"
        title="Client-side audio cache"
        actions={
          <button
            className="ghost-button"
            disabled={isClearing || !cacheStatus || cacheStatus.totalCachedTracks === 0}
            onClick={handleClearCache}
            type="button"
          >
            {isClearing ? 'Clearing...' : 'Clear cache'}
          </button>
        }
      >
        <p className="panel-copy">
          Aria downloads remote tracks atomically to local disk before playback to ensure bit-perfect,
          uninterrupted playback. A 3-track sliding window prefetches upcoming tracks in the background
          and automatically evicts older played tracks in LRU order.
        </p>

        <div className="metrics-grid">
          <div>
            <span>Cached Tracks</span>
            <strong>{cacheStatus ? cacheStatus.totalCachedTracks : '—'}</strong>
          </div>
          <div>
            <span>Disk Space Used</span>
            <strong>{cacheStatus ? formatBytes(cacheStatus.totalCachedBytes) : '—'}</strong>
          </div>
          <div>
            <span>Sliding Window</span>
            <strong>{cacheStatus ? `${cacheStatus.windowSize} tracks` : '3 tracks'}</strong>
          </div>
        </div>

        {cacheStatus?.cacheDir ? (
          <div style={{ marginTop: '0.8rem', fontSize: '0.75rem', color: 'var(--muted)' }}>
            <span>Cache directory: </span>
            <code style={{ background: 'var(--surface-sunken)', padding: '2px 6px', borderRadius: '4px' }}>
              {cacheStatus.cacheDir}
            </code>
          </div>
        ) : null}
      </SectionCard>
    </div>
  );
}

function formatBackendLabel(backend: RemoteBackendType): string {
  switch (backend) {
    case 'google_drive':
      return 'Google Drive';
    case 'smb':
      return 'SMB (Windows Share)';
    case 'web_dav':
      return 'WebDAV';
    case 'filesystem':
      return 'Local / Mounted Path';
    default:
      return backend;
  }
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
}

type AddRemoteDialogProps = {
  onClose: () => void;
  onAdded: (targets: RemoteTarget[]) => void;
};

function AddRemoteDialog({ onClose, onAdded }: AddRemoteDialogProps) {
  const [backendType, setBackendType] = useState<RemoteBackendType>('filesystem');
  const [name, setName] = useState('');
  const [storageLimitGb, setStorageLimitGb] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Filesystem config
  const [fsRootPath, setFsRootPath] = useState('');

  // WebDAV config
  const [webdavUrl, setWebdavUrl] = useState('');
  const [webdavUsername, setWebdavUsername] = useState('');
  const [webdavPassword, setWebdavPassword] = useState('');

  // SMB config
  const [smbSharePath, setSmbSharePath] = useState('');
  const [smbUsername, setSmbUsername] = useState('');
  const [smbPassword, setSmbPassword] = useState('');
  const [smbDomain, setSmbDomain] = useState('');

  // Google Drive config
  const [gdriveClientId, setGdriveClientId] = useState('');
  const [gdriveClientSecret, setGdriveClientSecret] = useState('');
  const [gdriveAuthUrl, setGdriveAuthUrl] = useState<string | null>(null);

  async function handleStartGdriveAuth() {
    if (!gdriveClientId.trim()) {
      setError('Please provide a Google OAuth Client ID');
      return;
    }
    setError(null);
    setSubmitting(true);
    try {
      const authUrl = await startGdriveAuthFlow(
        gdriveClientId.trim(),
        gdriveClientSecret.trim() || undefined,
      );
      setGdriveAuthUrl(authUrl);
      window.open(authUrl, '_blank');
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleCompleteGdriveAuth() {
    if (!name.trim()) {
      setError('Please provide a name for this remote target');
      return;
    }
    setError(null);
    setSubmitting(true);
    try {
      const limitBytes = storageLimitGb ? parseFloat(storageLimitGb) * 1024 * 1024 * 1024 : undefined;
      const target = await completeGdriveAuthFlow(name.trim(), limitBytes);
      const updated = await saveRemoteTarget(target);
      onAdded(updated);
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  async function handleSaveManual() {
    if (!name.trim()) {
      setError('Please enter a target name.');
      return;
    }

    let configJson = '{}';
    if (backendType === 'filesystem') {
      if (!fsRootPath.trim()) {
        setError('Please enter a root path.');
        return;
      }
      configJson = JSON.stringify({ root_path: fsRootPath.trim() });
    } else if (backendType === 'web_dav') {
      if (!webdavUrl.trim()) {
        setError('Please enter the WebDAV server URL.');
        return;
      }
      configJson = JSON.stringify({
        url: webdavUrl.trim(),
        username: webdavUsername.trim() || undefined,
        password: webdavPassword.trim() || undefined,
      });
    } else if (backendType === 'smb') {
      if (!smbSharePath.trim()) {
        setError('Please enter the SMB share path (e.g. \\\\server\\share).');
        return;
      }
      configJson = JSON.stringify({
        share_path: smbSharePath.trim(),
        username: smbUsername.trim() || undefined,
        password: smbPassword.trim() || undefined,
        domain: smbDomain.trim() || undefined,
      });
    }

    const limitBytes = storageLimitGb.trim()
      ? Math.round(parseFloat(storageLimitGb) * 1024 * 1024 * 1024)
      : null;

    const newTarget: RemoteTarget = {
      id: `target-${Date.now()}`,
      name: name.trim(),
      backendType,
      storageLimitBytes: limitBytes,
      isEnabled: true,
      configJson,
    };

    setSubmitting(true);
    setError(null);
    try {
      const updated = await saveRemoteTarget(newTarget);
      onAdded(updated);
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="add-remote-dialog-title"
        aria-modal="true"
        className="dialog-card"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Remote</p>
            <h3 id="add-remote-dialog-title">Add Remote Target</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        {error ? (
          <div className="error-banner" style={{ marginBottom: '1rem' }}>
            {error}
          </div>
        ) : null}

        <div className="field-stack">
          <label className="field-label" htmlFor="backend-type-select">
            Storage Provider
          </label>
          <select
            id="backend-type-select"
            value={backendType}
            onChange={(e) => setBackendType(e.target.value as RemoteBackendType)}
          >
            <option value="filesystem">Local / Mounted Path (Filesystem)</option>
            <option value="smb">SMB Network Share (Direct Windows Auth)</option>
            <option value="web_dav">WebDAV Server (Nextcloud, Synology, etc.)</option>
            <option value="google_drive">Google Drive (OAuth PKCE)</option>
          </select>
        </div>

        <div className="field-stack">
          <label className="field-label" htmlFor="target-name-input">
            Target Name
          </label>
          <input
            id="target-name-input"
            type="text"
            placeholder="e.g. Synology NAS, Google Drive Mobile"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div className="field-stack">
          <label className="field-label" htmlFor="storage-limit-input">
            Optional Storage Limit (GB)
          </label>
          <input
            id="storage-limit-input"
            type="number"
            placeholder="e.g. 50 (leave empty for unlimited)"
            value={storageLimitGb}
            onChange={(e) => setStorageLimitGb(e.target.value)}
          />
        </div>

        {backendType === 'filesystem' ? (
          <div className="field-stack">
            <label className="field-label" htmlFor="fs-root-input">
              Root Directory Path
            </label>
            <input
              id="fs-root-input"
              type="text"
              placeholder="e.g. D:\CloudSync\Aria or \\server\mount"
              value={fsRootPath}
              onChange={(e) => setFsRootPath(e.target.value)}
            />
          </div>
        ) : null}

        {backendType === 'web_dav' ? (
          <>
            <div className="field-stack">
              <label className="field-label" htmlFor="webdav-url-input">
                WebDAV URL
              </label>
              <input
                id="webdav-url-input"
                type="text"
                placeholder="e.g. https://nas.local:5006/music"
                value={webdavUrl}
                onChange={(e) => setWebdavUrl(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="webdav-user-input">
                Username
              </label>
              <input
                id="webdav-user-input"
                type="text"
                value={webdavUsername}
                onChange={(e) => setWebdavUsername(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="webdav-pass-input">
                Password
              </label>
              <input
                id="webdav-pass-input"
                type="password"
                value={webdavPassword}
                onChange={(e) => setWebdavPassword(e.target.value)}
              />
            </div>
          </>
        ) : null}

        {backendType === 'smb' ? (
          <>
            <div className="field-stack">
              <label className="field-label" htmlFor="smb-share-input">
                SMB Share Path (UNC)
              </label>
              <input
                id="smb-share-input"
                type="text"
                placeholder="e.g. \\192.168.1.100\Music"
                value={smbSharePath}
                onChange={(e) => setSmbSharePath(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="smb-user-input">
                Username (Optional if already logged in)
              </label>
              <input
                id="smb-user-input"
                type="text"
                placeholder="username"
                value={smbUsername}
                onChange={(e) => setSmbUsername(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="smb-pass-input">
                Password
              </label>
              <input
                id="smb-pass-input"
                type="password"
                value={smbPassword}
                onChange={(e) => setSmbPassword(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="smb-domain-input">
                Domain / Workgroup (Optional)
              </label>
              <input
                id="smb-domain-input"
                type="text"
                placeholder="WORKGROUP"
                value={smbDomain}
                onChange={(e) => setSmbDomain(e.target.value)}
              />
            </div>
          </>
        ) : null}

        {backendType === 'google_drive' ? (
          <>
            <div className="field-stack">
              <label className="field-label" htmlFor="gdrive-client-id">
                Google OAuth Client ID
              </label>
              <input
                id="gdrive-client-id"
                type="text"
                placeholder="xxxx.apps.googleusercontent.com"
                value={gdriveClientId}
                onChange={(e) => setGdriveClientId(e.target.value)}
              />
            </div>
            <div className="field-stack">
              <label className="field-label" htmlFor="gdrive-client-secret">
                Google OAuth Client Secret (Optional)
              </label>
              <input
                id="gdrive-client-secret"
                type="password"
                placeholder="Optional for desktop PKCE apps"
                value={gdriveClientSecret}
                onChange={(e) => setGdriveClientSecret(e.target.value)}
              />
            </div>

            {!gdriveAuthUrl ? (
              <button
                className="ghost-button"
                disabled={submitting || !gdriveClientId.trim()}
                onClick={() => void handleStartGdriveAuth()}
                type="button"
                style={{ marginTop: '0.5rem' }}
              >
                {submitting ? 'Starting Auth Server...' : 'Sign in with Google'}
              </button>
            ) : (
              <div style={{ display: 'grid', gap: '0.5rem', marginTop: '0.5rem' }}>
                <p className="dialog-section__note">
                  Browser window opened for Google login. Complete authentication in your browser,
                  then click "Confirm Authorization" below.
                </p>
                <button
                  disabled={submitting || !name.trim()}
                  onClick={() => void handleCompleteGdriveAuth()}
                  type="button"
                >
                  {submitting ? 'Saving...' : 'Confirm Authorization & Save'}
                </button>
              </div>
            )}
          </>
        ) : (
          <div style={{ marginTop: '1.2rem', display: 'flex', justifyContent: 'flex-end', gap: '0.6rem' }}>
            <button className="ghost-button" onClick={onClose} type="button">
              Cancel
            </button>
            <button disabled={submitting} onClick={() => void handleSaveManual()} type="button">
              {submitting ? 'Saving...' : 'Save Remote Target'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
