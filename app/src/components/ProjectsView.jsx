import { useDeferredValue, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { VirtualList } from './VirtualList.jsx';
import { age, bytes, nowSecs, DAY_SECS } from '../lib/format.js';

const TIERS = [
  { id: 'safe', label: 'Safe', hint: 'lockfile / pure cache — exact reinstall' },
  { id: 'rebuild', label: 'Rebuild', hint: 'build output — costs a rebuild' },
  { id: 'heavy', label: 'Heavy', hint: 'regenerable but slow (Unity Library, UE DDC)' },
];
const AGES = [0, 7, 14, 30, 90, 180];
const ROW_H = 46;

/** Heavy items are opt-in; everything else starts checked. */
const defaultSelected = (it) => it.tier !== 'heavy';

const NO_OVERRIDES = new Map();

export function ProjectsView({ items, scanId, progress, onDelete }) {
  const [tiers, setTiers] = useState({ safe: true, rebuild: true, heavy: false });
  const [minAge, setMinAge] = useState(0);
  const [query, setQuery] = useState('');
  const [eco, setEco] = useState('all');
  // Only user deviations from the default are stored. "Is X selected" is derived,
  // so newly streamed rows are selected/unselected correctly with zero syncing.
  // Stamped with scanId: ids restart per scan, so a stale map is simply ignored
  // (derived during render) instead of being cleared by a reset effect.
  const [overrides, setOverrides] = useState({ scanId, map: NO_OVERRIDES });
  const overrideMap = overrides.scanId === scanId ? overrides.map : NO_OVERRIDES;

  // Filtering 10k+ rows on every keystroke: React renders the input urgently and the
  // list at lower priority. Defer each value separately — deferring a fresh object
  // literal would yield a new identity every render and bust the memo below.
  const dItems = useDeferredValue(items);
  const dTiers = useDeferredValue(tiers);
  const dMinAge = useDeferredValue(minAge);
  const dQuery = useDeferredValue(query);
  const dEco = useDeferredValue(eco);

  const isSelected = (it) => overrideMap.get(it.id) ?? defaultSelected(it);

  const ecosystems = useMemo(() => [...new Set(dItems.map((i) => i.ecosystem))].sort(), [dItems]);

  const visible = useMemo(() => {
    const now = nowSecs();
    const q = dQuery.trim().toLowerCase();
    return dItems
      .filter(
        (it) =>
          dTiers[it.tier] &&
          (dEco === 'all' || it.ecosystem === dEco) &&
          (dMinAge === 0 || now - it.modified >= dMinAge * DAY_SECS) &&
          (!q || it.path.toLowerCase().includes(q)),
      )
      .sort((a, b) => b.bytes - a.bytes);
  }, [dItems, dTiers, dMinAge, dQuery, dEco]);

  // Per-tier totals over ALL items (not just visible) so toggles show what they hide.
  const tierStats = useMemo(() => {
    const s = { safe: [0, 0], rebuild: [0, 0], heavy: [0, 0] };
    for (const it of dItems) {
      s[it.tier][0]++;
      s[it.tier][1] += it.bytes;
    }
    return s;
  }, [dItems]);

  // What-you-see-is-what-gets-deleted: selection is intersected with the filters.
  const toDelete = visible.filter(isSelected);
  const toDeleteBytes = toDelete.reduce((n, it) => n + it.bytes, 0);
  const allChecked = toDelete.length === visible.length && visible.length > 0;

  const setMany = (list, value) =>
    setOverrides((prev) => {
      const next = new Map(prev.scanId === scanId ? prev.map : undefined);
      for (const it of list) next.set(it.id, value);
      return { scanId, map: next };
    });

  const now = nowSecs();
  const scanning = progress.status === 'scanning';

  return (
    <>
      <div className="filters">
        <div className="tiers">
          {TIERS.map((t) => (
            <label key={t.id} className={`tier-toggle ${t.id} ${tiers[t.id] ? 'on' : ''}`} title={t.hint}>
              <input
                type="checkbox"
                checked={tiers[t.id]}
                onChange={(e) => setTiers((p) => ({ ...p, [t.id]: e.target.checked }))}
              />
              {t.label}
              <small>
                {tierStats[t.id][0]} · {bytes(tierStats[t.id][1])}
              </small>
            </label>
          ))}
        </div>
        <select value={eco} onChange={(e) => setEco(e.target.value)} aria-label="Ecosystem">
          <option value="all">All ecosystems</option>
          {ecosystems.map((e) => (
            <option key={e}>{e}</option>
          ))}
        </select>
        <select value={minAge} onChange={(e) => setMinAge(Number(e.target.value))} aria-label="Untouched for">
          {AGES.map((d) => (
            <option key={d} value={d}>
              {d === 0 ? 'Any age' : `Untouched ≥ ${d} days`}
            </option>
          ))}
        </select>
        <input
          className="search"
          placeholder="Filter by path…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>

      <div className="list-head">
        <input
          type="checkbox"
          checked={allChecked}
          onChange={(e) => setMany(visible, e.target.checked)}
          aria-label="Select all visible"
        />
        <span>Folder</span>
        <span className="r">Last touched</span>
        <span className="r">Size</span>
      </div>

      <VirtualList
        items={visible}
        rowHeight={ROW_H}
        empty={
          scanning ? 'Looking…' : progress.status === 'idle' ? 'Pick drives or folders above, then Scan.' : 'Nothing matches.'
        }
        renderRow={(it, top) => (
          <div key={it.id} className={`row ${isSelected(it) ? 'sel' : ''}`} style={{ top, height: ROW_H }}>
            <input type="checkbox" checked={isSelected(it)} onChange={(e) => setMany([it], e.target.checked)} />
            <div className="main" title={`${it.path}\nRestore: ${it.restore}`}>
              <div className="name">
                <span className={`badge ${it.tier}`}>{it.tier}</span>
                <b>{it.name}</b>
                <span className="eco">{it.ecosystem}</span>
              </div>
              <div className="path">{it.project}</div>
            </div>
            <span className="r dim">{age(it.modified, now)}</span>
            <span className="r size">{bytes(it.bytes)}</span>
            <button className="icon" title="Show in Explorer" onClick={() => invoke('reveal', { path: it.path })}>
              ↗
            </button>
          </div>
        )}
      />

      <footer className="bar">
        <span className="dim">
          {visible.length} shown · {toDelete.length} selected
        </span>
        <button className="btn danger" disabled={toDelete.length === 0 || scanning} onClick={() => onDelete(toDelete)}>
          Delete {bytes(toDeleteBytes)}
        </button>
      </footer>
    </>
  );
}
