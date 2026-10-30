import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { bytes } from '../lib/format.js';

export function CachesView({ onToast, onCleaned }) {
  // null = not detected yet; [] = detected, none found.
  const [caches, setCaches] = useState(null);
  const [busy, setBusy] = useState(false);
  const [overrides, setOverrides] = useState(() => new Map());

  const isSelected = (c) => overrides.get(c.id) ?? c.tier !== 'heavy';
  const chosen = (caches ?? []).filter(isSelected);
  const chosenBytes = chosen.reduce((n, c) => n + c.bytes, 0);

  // Detection runs on click, not on mount: sizing every cache costs a few seconds of
  // disk I/O the user didn't ask for yet.
  const detect = async () => {
    setBusy(true);
    try {
      setCaches(await invoke('list_caches'));
      setOverrides(new Map());
    } finally {
      setBusy(false);
    }
  };

  const clean = async () => {
    setBusy(true);
    try {
      const results = await invoke('clean_caches', { ids: chosen.map((c) => c.id) });
      const freed = results.reduce((n, r) => n + r.freed, 0);
      const locked = results.filter((r) => r.errors.length > 0).map((r) => r.id);
      onCleaned();
      onToast(`Freed ${bytes(freed)} from caches${locked.length ? ` · some files locked in: ${locked.join(', ')}` : ''}`);
      setCaches(await invoke('list_caches'));
    } finally {
      setBusy(false);
    }
  };

  if (caches === null) {
    return (
      <div className="caches-intro">
        <p>
          Global package-manager and toolchain caches (npm, pnpm, Cargo, NuGet, Gradle, pip, Go, Playwright…).
          Everything here re-downloads on demand.
        </p>
        <button className="btn primary" onClick={detect} disabled={busy}>
          {busy ? 'Measuring…' : 'Detect caches'}
        </button>
      </div>
    );
  }

  return (
    <>
      <div className="cache-list">
        {caches.length === 0 && <div className="empty">No caches found.</div>}
        {caches.map((c) => (
          <label key={c.id} className={`cache ${isSelected(c) ? 'sel' : ''}`}>
            <input
              type="checkbox"
              checked={isSelected(c)}
              onChange={(e) => setOverrides((m) => new Map(m).set(c.id, e.target.checked))}
            />
            <div className="main">
              <div className="name">
                <span className={`badge ${c.tier}`}>{c.tier}</span>
                <b>{c.name}</b>
                <span className="eco">{c.method}</span>
              </div>
              <div className="path">{c.note || c.paths.join(' · ')}</div>
            </div>
            <span className="r size">{bytes(c.bytes)}</span>
          </label>
        ))}
      </div>
      <footer className="bar">
        <button className="btn ghost" onClick={detect} disabled={busy}>
          Re-measure
        </button>
        <button className="btn danger" disabled={busy || chosen.length === 0} onClick={clean}>
          {busy ? 'Working…' : `Clean ${bytes(chosenBytes)}`}
        </button>
      </footer>
    </>
  );
}
