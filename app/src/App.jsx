import { useEffect, useState } from 'react';
import { invoke, Channel } from '@tauri-apps/api/core';
import { useScan } from './hooks/useScan.js';
import { TargetBar } from './components/TargetBar.jsx';
import { ProjectsView } from './components/ProjectsView.jsx';
import { CachesView } from './components/CachesView.jsx';
import { ConfirmDialog } from './components/ConfirmDialog.jsx';
import { bytes } from './lib/format.js';

export default function App() {
  const [roots, setRoots] = useState([]);
  const [tab, setTab] = useState('projects');
  const [pending, setPending] = useState(null); // items awaiting confirmation
  const [deleting, setDeleting] = useState(null); // { done, total, freed } while purging
  const [toast, setToast] = useState(null);
  const { items, scanId, progress, start, cancel, removeIds } = useScan();

  // Auto-dismiss: a timer is an external side effect bound to the toast's lifetime.
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast]);

  const runDelete = (list) => {
    setPending(null);
    setDeleting({ done: 0, total: list.length, freed: 0 });
    const ch = new Channel();
    ch.onmessage = (ev) => {
      if (ev.type === 'staged') {
        // Folders are already renamed away on disk: drop the rows right now; the
        // byte-level purge continues in the background.
        removeIds(ev.ids);
      } else if (ev.type === 'progress') {
        setDeleting({ done: ev.done, total: ev.total, freed: ev.freed });
      } else if (ev.type === 'done') {
        setDeleting(null);
        setToast(
          `Freed ${bytes(ev.freed)}` +
            (ev.errors.length ? ` · ${ev.errors.length} item(s) locked or in use — close the app using them and rescan` : ''),
        );
      }
    };
    invoke('delete_items', { ids: list.map((i) => i.id), onEvent: ch });
  };

  const scanning = progress.status === 'scanning';
  const total = items.reduce((n, it) => n + it.bytes, 0);

  return (
    <div className="app">
      <header className="top">
        <div className="brand">
          <h1>devClean</h1>
          <span className="dim">re-creatable dev artifacts only</span>
        </div>
        <div className="stats">
          {scanning && (
            <>
              <span className="pulse" /> {progress.dirs.toLocaleString()} dirs · {progress.found} found ·{' '}
              <b>{bytes(progress.bytes)}</b>
            </>
          )}
          {(progress.status === 'done' || progress.status === 'cancelled') && (
            <>
              {progress.status === 'cancelled' ? 'Cancelled · ' : ''}
              {progress.dirs.toLocaleString()} dirs in {(progress.ms / 1000).toFixed(2)}s ·{' '}
              <b>{bytes(total)}</b> reclaimable
            </>
          )}
          {progress.status === 'error' && <span className="err">{progress.error}</span>}
        </div>
      </header>

      <TargetBar
        roots={roots}
        setRoots={setRoots}
        scanning={scanning}
        onScan={() => {
          setTab('projects');
          start(roots);
        }}
        onCancel={cancel}
      />

      <nav className="tabs">
        <button className={tab === 'projects' ? 'on' : ''} onClick={() => setTab('projects')}>
          Projects {items.length > 0 && <small>{items.length}</small>}
        </button>
        <button className={tab === 'caches' ? 'on' : ''} onClick={() => setTab('caches')}>
          Global caches
        </button>
        {deleting && (
          <span className="deleting">
            Deleting {deleting.done}/{deleting.total} · {bytes(deleting.freed)} freed
          </span>
        )}
      </nav>

      <main className="content">
        {/* Both views stay mounted: switching tabs keeps filters and selection. */}
        <div className="view" hidden={tab !== 'projects'}>
          <ProjectsView items={items} scanId={scanId} progress={progress} onDelete={setPending} />
        </div>
        <div className="view" hidden={tab !== 'caches'}>
          <CachesView onToast={setToast} />
        </div>
      </main>

      {pending && <ConfirmDialog items={pending} onConfirm={() => runDelete(pending)} onClose={() => setPending(null)} />}
      {toast && <div className="toast" onClick={() => setToast(null)}>{toast}</div>}
    </div>
  );
}
