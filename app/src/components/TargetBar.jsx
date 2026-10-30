import { open } from '@tauri-apps/plugin-dialog';
import { bytes } from '../lib/format.js';

const norm = (p) => p.toLowerCase().replace(/[\\/]+$/, '');

// Below this, "+freed" is noise from temp files and browser caches churning.
const GAIN_MIN = 1 << 20;

export function TargetBar({ roots, setRoots, drives, baseline, scanning, onScan, onCancel }) {
  const inList = (list, p) => list.some((r) => norm(r) === norm(p));
  const has = (p) => inList(roots, p);
  // Decide inside the updater from `rs`, not from the render-time `roots` closure.
  const toggle = (p) => setRoots((rs) => (inList(rs, p) ? rs.filter((r) => norm(r) !== norm(p)) : [...rs, p]));

  const addFolders = async () => {
    const picked = await open({ directory: true, multiple: true, title: 'Choose folders to scan' });
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    setRoots((rs) => [...rs, ...list.filter((p) => !inList(rs, p))]);
  };

  const folders = roots.filter((r) => !drives.some((d) => norm(d.path) === norm(r)));

  return (
    <section className="targets">
      <div className="drives">
        {drives.map((d) => {
          const used = 1 - d.free / d.total;
          // Derived per render from the live drive list vs the first reading.
          const gained = d.free - (baseline?.get(d.path) ?? d.free);
          return (
            <button
              key={d.path}
              className={`drive ${has(d.path) ? 'on' : ''}`}
              onClick={() => toggle(d.path)}
              disabled={scanning}
              title={`${bytes(d.free)} free of ${bytes(d.total)}`}
            >
              <span className="letter">{d.path.slice(0, 2)}</span>
              <span className="usage"><i style={{ width: `${used * 100}%` }} /></span>
              <span className="free">
                {bytes(d.free)} free
                {gained >= GAIN_MIN && <em className="gain"> +{bytes(gained)}</em>}
              </span>
            </button>
          );
        })}
        <button className="drive add" onClick={addFolders} disabled={scanning}>+ Folder…</button>
      </div>

      {folders.length > 0 && (
        <div className="chips">
          {folders.map((f) => (
            <span key={f} className="chip">
              {f}
              {!scanning && <button onClick={() => toggle(f)} aria-label={`remove ${f}`}>×</button>}
            </span>
          ))}
        </div>
      )}

      <div className="scan-actions">
        {scanning ? (
          <button className="btn ghost" onClick={onCancel}>Cancel</button>
        ) : (
          <button className="btn primary" onClick={onScan} disabled={roots.length === 0}>
            Scan {roots.length > 0 ? `${roots.length} target${roots.length > 1 ? 's' : ''}` : ''}
          </button>
        )}
      </div>
    </section>
  );
}
