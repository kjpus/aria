import { useEffect, useState } from 'react';
import type { UploadFailedItem } from '../../types/aria';
import { copyToClipboard } from '../../lib/aria';

type RemoteUploadFailureDialogProps = {
  failedItems: UploadFailedItem[];
  onClose: () => void;
};

export function RemoteUploadFailureDialog({
  failedItems,
  onClose,
}: RemoteUploadFailureDialogProps) {
  const [hasCopied, setHasCopied] = useState(false);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  async function handleCopyList() {
    const lines = failedItems.map(
      (item) => `${item.title}\n(${item.path})\nError: ${item.error}`
    );
    const text = `Upload Failed Items (${failedItems.length}):\n\n${lines.join('\n\n')}`;
    await copyToClipboard(text);
    setHasCopied(true);
    setTimeout(() => setHasCopied(false), 2500);
  }

  return (
    <div className="dialog-backdrop" onClick={onClose} role="presentation">
      <div
        aria-labelledby="upload-failure-dialog-title"
        aria-modal="true"
        className="dialog-card dialog-card--wide"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="dialog-card__header">
          <div>
            <p className="section-card__eyebrow">Remote Upload</p>
            <h3 id="upload-failure-dialog-title">Upload Finished with Errors</h3>
          </div>
          <button className="ghost-button" onClick={onClose} type="button">
            Close
          </button>
        </div>

        <p className="dialog-card__copy">
          The upload process completed, but <strong>{failedItems.length}</strong> track
          {failedItems.length === 1 ? '' : 's'} could not be uploaded. Successful tracks were
          saved and synced to your remote storage.
        </p>

        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', margin: '1rem 0 0.6rem' }}>
          <span
            className="pane-chip"
            style={{
              backgroundColor: 'rgba(239, 68, 68, 0.15)',
              color: '#f87171',
              borderColor: 'rgba(239, 68, 68, 0.3)',
            }}
          >
            {failedItems.length} failed item{failedItems.length === 1 ? '' : 's'}
          </span>

          <button
            className="ghost-button"
            onClick={() => void handleCopyList()}
            style={{ fontSize: '0.82rem', padding: '0.35rem 0.75rem' }}
            type="button"
          >
            {hasCopied ? '✓ Copied to Clipboard!' : 'Copy Failed List'}
          </button>
        </div>

        <div
          style={{
            maxHeight: '360px',
            overflowY: 'auto',
            border: '1px solid var(--line)',
            borderRadius: '16px',
            background: 'rgba(0, 0, 0, 0.2)',
            padding: '0.25rem 0.5rem',
            marginBottom: '1.2rem',
          }}
        >
          <table style={{ width: '100%', borderCollapse: 'collapse', textAlign: 'left' }}>
            <thead>
              <tr style={{ borderBottom: '1px solid var(--line)' }}>
                <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)', width: '80px' }}>
                  Status
                </th>
                <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)', width: '30%' }}>
                  Track Title
                </th>
                <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)', width: '35%' }}>
                  File Path
                </th>
                <th style={{ padding: '0.6rem 0.5rem', fontSize: '0.85rem', color: 'var(--muted)', width: '25%' }}>
                  Error Reason
                </th>
              </tr>
            </thead>
            <tbody>
              {failedItems.map((item, idx) => (
                <tr key={idx} style={{ borderBottom: '1px solid rgba(255, 255, 255, 0.04)' }}>
                  <td style={{ padding: '0.6rem 0.5rem', verticalAlign: 'top' }}>
                    <span
                      className="pane-chip"
                      style={{
                        fontSize: '0.78rem',
                        padding: '0.15rem 0.45rem',
                        backgroundColor: 'rgba(239, 68, 68, 0.15)',
                        color: '#f87171',
                        borderColor: 'rgba(239, 68, 68, 0.25)',
                      }}
                    >
                      Failed
                    </span>
                  </td>
                  <td
                    style={{
                      padding: '0.6rem 0.5rem',
                      verticalAlign: 'top',
                      color: 'var(--text)',
                      fontSize: '0.88rem',
                      fontWeight: 500,
                      wordBreak: 'break-word',
                    }}
                    title={item.title}
                  >
                    {item.title}
                  </td>
                  <td
                    style={{
                      padding: '0.6rem 0.5rem',
                      verticalAlign: 'top',
                      color: 'var(--muted)',
                      fontSize: '0.8rem',
                      wordBreak: 'break-all',
                      fontFamily: 'monospace',
                    }}
                    title={item.path}
                  >
                    {item.path}
                  </td>
                  <td
                    style={{
                      padding: '0.6rem 0.5rem',
                      verticalAlign: 'top',
                      color: '#fca5a5',
                      fontSize: '0.82rem',
                      wordBreak: 'break-word',
                    }}
                    title={item.error}
                  >
                    {item.error}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '0.6rem' }}>
          <button
            className="ghost-button"
            onClick={() => void handleCopyList()}
            type="button"
          >
            {hasCopied ? '✓ Copied!' : 'Copy List'}
          </button>
          <button onClick={onClose} type="button">
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
