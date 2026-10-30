import { bytes } from '../lib/format.js';

const clamp01 = (x) => Math.min(1, Math.max(0, x || 0));

const duration = (ms) => {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
};

/**
 * Live view of a delete run. Everything shown is derived from the job snapshot the
 * backend streams every 100 ms — no local timers, no duplicated counters.
 */
export function DeletePanel({ job, onHide, onClose }) {
  const done = job.phase === 'done';
  const secs = job.ms / 1000;
  // Delete time scales with file count (one syscall per file), not bytes, so progress
  // and ETA are file-based; bytes are what the user cares about, so those are shown big.
  const pct = done ? 1 : clamp01(job.totalFiles ? job.files / job.totalFiles : job.freed / job.totalBytes);
  const filesPerSec = secs > 0.3 ? job.files / secs : 0;
  const eta = filesPerSec > 0 ? Math.max(0, job.totalFiles - job.files) / filesPerSec : null;
  const queued = Math.max(0, job.total - job.finished.length - job.active.length);

  return (
    <div className="del-backdrop">
      <section className={`del-panel ${done ? 'done' : ''}`} role="status" aria-live="polite">
        <header>
          <div>
            <h2>
              {done
                ? `Freed ${bytes(job.freed)}`
                : job.phase === 'staging'
                  ? `Preparing ${job.total} folders…`
                  : `Deleting ${job.total} folder${job.total === 1 ? '' : 's'}`}
            </h2>
            <span className="dim">
              {done
                ? `${job.files.toLocaleString()} files removed in ${duration(job.ms)}`
                : 'Folders are already renamed out of your projects; their files are being removed now.'}
            </span>
          </div>
          {done ? (
            <button className="btn primary" onClick={onClose}>Done</button>
          ) : (
            <button className="btn ghost" onClick={onHide} title="Keep deleting in the background">Hide</button>
          )}
        </header>

        <div className="del-meter">
          <i style={{ width: `${pct * 100}%` }} />
          <span>{Math.floor(pct * 100)}%</span>
        </div>

        <dl className="del-stats">
          <div>
            <dt>Freed</dt>
            <dd>{bytes(job.freed)} <small>/ {bytes(job.totalBytes)}</small></dd>
          </div>
          <div>
            <dt>Files</dt>
            <dd>{job.files.toLocaleString()} <small>/ {job.totalFiles.toLocaleString()}</small></dd>
          </div>
          <div>
            <dt>Speed</dt>
            <dd>
              {secs > 0.3 ? `${bytes(Math.round(job.freed / secs))}/s` : '—'}{' '}
              <small>{filesPerSec ? `${Math.round(filesPerSec).toLocaleString()} files/s` : ''}</small>
            </dd>
          </div>
          <div>
            <dt>{done ? 'Took' : 'Left'}</dt>
            <dd>
              {done ? duration(job.ms) : eta != null ? `~${duration(eta * 1000)}` : '—'}{' '}
              <small>{done ? '' : `${duration(job.ms)} elapsed`}</small>
            </dd>
          </div>
        </dl>

        {job.active.length > 0 && (
          <>
            <h3>Now deleting <small>{job.active.length}</small></h3>
            <ul className="del-active">
              {job.active.map((a) => {
                const it = job.byId.get(a.id);
                if (!it) return null;
                return (
                  <li key={a.id}>
                    <div className="line">
                      <b>{it.name}</b>
                      <span className="path">{it.project}</span>
                      <span className="size">{bytes(a.freed)} <small>/ {bytes(it.bytes)}</small></span>
                    </div>
                    <div className="mini">
                      <i style={{ width: `${clamp01(it.files ? a.files / it.files : 0) * 100}%` }} />
                    </div>
                    {/* The sub-folder being emptied at this moment — the "what is it doing" answer. */}
                    {/* rtl + <bdi>: overflow trims the START of the path, keeping the
                        deepest (most informative) segments visible. */}
                    <code className="current">
                      <bdi>{a.current ?? '…'}</bdi>
                    </code>
                  </li>
                );
              })}
            </ul>
          </>
        )}

        <h3>
          Completed <small>{job.finished.length}/{job.total}</small>
          {queued > 0 && !done && <span className="dim"> · {queued} queued</span>}
          {job.failed.length > 0 && <span className="err"> · {job.failed.length} refused</span>}
        </h3>
        <ul className="del-finished">
          {job.finished.map((f) => {
            const it = job.byId.get(f.id);
            if (!it) return null;
            return (
              <li key={f.id} className={f.errors ? 'warn' : ''}>
                <span className="tick">{f.errors ? '!' : '✓'}</span>
                <b>{it.name}</b>
                <span className="path">{it.project}</span>
                <span className="size">{bytes(f.freed)}</span>
              </li>
            );
          })}
          {job.finished.length === 0 && <li className="dim">Nothing finished yet…</li>}
        </ul>

        {done && job.errors.length > 0 && (
          <details className="del-errors">
            <summary>
              {job.errors.length} item(s) could not be removed — locked or in use. Close the program using them and rescan.
            </summary>
            <pre>{job.errors.slice(0, 50).join('\n')}</pre>
          </details>
        )}
      </section>
    </div>
  );
}
