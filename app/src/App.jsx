import { useEffect, useState } from 'react';
import { useScan } from './hooks/useScan.js';
import { useDrives } from './hooks/useDrives.js';
import { useDelete } from './hooks/useDelete.js';
import { TitleBar } from './components/TitleBar.jsx';
import { TargetBar } from './components/TargetBar.jsx';
import { ProjectsView } from './components/ProjectsView.jsx';
import { CachesView } from './components/CachesView.jsx';
import { ConfirmDialog } from './components/ConfirmDialog.jsx';
import { DeletePanel } from './components/DeletePanel.jsx';
import { bytes } from './lib/format.js';

// Free-space re-read cadence while files are being removed. GetDiskFreeSpaceExW is a
// cheap kernel query, so twice a second is nothing — and it makes the drive meters move.
const DRIVE_POLL_MS = 500;

export default function App() {
  const [roots, setRoots] = useState([]);
  const [tab, setTab] = useState('projects');
  const [pending, setPending] = useState(null); // items awaiting confirmation
  const [panelHidden, setPanelHidden] = useState(false);
  const [toast, setToast] = useState(null);
  const { items, scanId, progress, start, cancel, removeIds } = useScan();
  const { drives, baseline, refresh: refreshDrives } = useDrives();
  const { job, start: startDelete, clear: clearDelete } = useDelete();

  const deleting = job !== null && job.phase !== 'done';

  // Auto-dismiss: a timer is an external side effect bound to the toast's lifetime.
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast]);

  // Poll free space only while a delete is running; keyed on the boolean, not on `job`,
  // so the 10 Hz progress updates don't tear down and recreate the interval.
  useEffect(() => {
    if (!deleting) return;
    const t = setInterval(refreshDrives, DRIVE_POLL_MS);
    return () => clearInterval(t);
  }, [deleting, refreshDrives]);

  const runDelete = (list) => {
    setPending(null);
    setPanelHidden(false);
    startDelete(list, {
      onStaged: removeIds,
      onDone: (ev) => {
        refreshDrives();
        setToast(
          `Freed ${bytes(ev.freed)}` +
            (ev.errors.length ? ` · ${ev.errors.length} item(s) locked or in use — close the app using them and rescan` : ''),
        );
      },
    });
  };

  const scanning = progress.status === 'scanning';
  const total = items.reduce((n, it) => n + it.bytes, 0);

  return (
    <div className="app">
      <TitleBar>
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
      </TitleBar>

      <TargetBar
        roots={roots}
        setRoots={setRoots}
        drives={drives}
        baseline={baseline}
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
        {/* Minimized delete panel: a live pill that reopens it. */}
        {job && panelHidden && (
          <button className="del-pill" onClick={() => setPanelHidden(false)}>
            <i style={{ width: `${job.totalFiles ? Math.min(1, job.files / job.totalFiles) * 100 : 0}%` }} />
            <span>
              {deleting ? `Deleting · ${bytes(job.freed)} freed` : `✓ Freed ${bytes(job.freed)}`}
            </span>
          </button>
        )}
      </nav>

      <main className="content">
        {/* Both views stay mounted: switching tabs keeps filters and selection. */}
        <div className="view" hidden={tab !== 'projects'}>
          <ProjectsView
            items={items}
            scanId={scanId}
            progress={progress}
            busy={deleting}
            onDelete={setPending}
          />
        </div>
        <div className="view" hidden={tab !== 'caches'}>
          <CachesView onToast={setToast} onCleaned={refreshDrives} />
        </div>
      </main>

      {pending && <ConfirmDialog items={pending} onConfirm={() => runDelete(pending)} onClose={() => setPending(null)} />}
      {job && !panelHidden && (
        <DeletePanel job={job} onHide={() => setPanelHidden(true)} onClose={clearDelete} />
      )}
      {/* Derived, not decided inside onDone: that closure was created at delete start and
          would read a stale `panelHidden`. The open panel already shows the result. */}
      {toast && !(job && !panelHidden) && (
        <div className="toast" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}
