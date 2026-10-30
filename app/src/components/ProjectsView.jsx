import { useDeferredValue, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { VirtualList } from './VirtualList.jsx';
import { age, bytes, nowSecs, DAY_SECS } from '../lib/format.js';
import { categorize, GROUP_MODES } from '../lib/categorize.js';

const TIERS = [
  { id: 'safe', label: 'Safe', hint: 'lockfile / pure cache — exact reinstall' },
  { id: 'rebuild', label: 'Rebuild', hint: 'build output — costs a rebuild' },
  { id: 'heavy', label: 'Heavy', hint: 'regenerable but slow (Unity Library, UE DDC)' },
];
const AGES = [0, 7, 14, 30, 90, 180];
const ROW_H = 46;
const INDENT = 22;
// Tree depth -> left padding; base matches the flat list's 20px gutter.
const indent = (depth) => ({ paddingLeft: 20 + depth * INDENT });

/** Heavy items are opt-in; everything else starts checked. */
const defaultSelected = (it) => it.tier !== 'heavy';

const NO_OVERRIDES = new Map();
const NO_OPEN = new Set();

export function ProjectsView({ items, scanId, progress, busy, onDelete }) {
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

  // Categorize: null = flat list, otherwise a GROUP_MODES id.
  const [groupBy, setGroupBy] = useState(null);
  // Expanded groups, stamped the same way: a new scan or mode yields new group keys,
  // so an old set is ignored by derivation rather than reset by an effect.
  const openStamp = `${scanId}:${groupBy}`;
  const [open, setOpen] = useState({ stamp: openStamp, set: NO_OPEN });
  const openSet = open.stamp === openStamp ? open.set : NO_OPEN;

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

  const scanning = progress.status === 'scanning';
  // Grouping is a post-scan view: while rows still stream in, groups would reshuffle
  // under the cursor every 60 ms. The choice is remembered and re-applies when done.
  const grouped = groupBy !== null && !scanning;
  const dGroupBy = useDeferredValue(grouped ? groupBy : null);

  const groups = useMemo(
    () => (dGroupBy ? categorize(visible, dGroupBy) : null),
    [visible, dGroupBy],
  );

  // The virtual list takes one flat array: the group tree is flattened depth-first,
  // descending only into expanded groups. Same row height for headers and items keeps
  // the virtualizer's O(1) offset math.
  const rows = useMemo(() => {
    if (!groups) return visible;
    const out = [];
    const walk = (gs, own, depth) => {
      for (const g of gs) {
        // A folder holding exactly one result IS that result: skip the pointless click.
        if (g.children.length === 0 && g.own.length === 1) {
          out.push({ item: g.own[0], depth });
          continue;
        }
        out.push({ group: g, depth });
        if (openSet.has(g.key)) walk(g.children, g.own, depth + 1);
      }
      for (const it of own) out.push({ item: it, depth });
    };
    walk(groups.groups, groups.own, 0);
    return out;
  }, [groups, visible, openSet]);

  // Updater reads `prev`, never the render-time `openSet`, so rapid clicks compose.
  const setOpenKeys = (fn) =>
    setOpen((prev) => ({ stamp: openStamp, set: fn(prev.stamp === openStamp ? prev.set : NO_OPEN) }));
  const toggleGroup = (key) =>
    setOpenKeys((s) => {
      const next = new Set(s);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  const allKeys = useMemo(() => {
    const keys = [];
    const walk = (gs) => gs.forEach((g) => (keys.push(g.key), walk(g.children)));
    if (groups) walk(groups.groups);
    return keys;
  }, [groups]);
  const allOpen = allKeys.length > 0 && allKeys.every((k) => openSet.has(k));

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
  // Collapsed groups still count — they are hidden rows, not filtered-out rows.
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
  const visibleBytes = groups ? visible.reduce((n, it) => n + it.bytes, 0) : 0;

  const renderItem = (it, top, depth) => (
    <div
      key={it.id}
      className={`row ${isSelected(it) ? 'sel' : ''}`}
      style={{ top, height: ROW_H, ...indent(depth) }}
    >
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
  );

  const renderGroup = (g, top, depth) => {
    const sel = g.items.filter(isSelected).length;
    const isOpen = openSet.has(g.key);
    return (
      <div
        key={`g:${g.key}`}
        className={`row group ${isOpen ? 'open' : ''} ${sel ? 'sel' : ''}`}
        style={{ top, height: ROW_H, ...indent(depth) }}
        onClick={() => toggleGroup(g.key)}
      >
        <input
          type="checkbox"
          checked={sel === g.items.length}
          // `indeterminate` has no HTML attribute; a callback ref sets the DOM property.
          ref={(el) => {
            if (el) el.indeterminate = sel > 0 && sel < g.items.length;
          }}
          onClick={(e) => e.stopPropagation()}
          onChange={(e) => setMany(g.items, e.target.checked)}
          aria-label={`Select all in ${g.label}`}
        />
        <div className="main" title={g.hint || g.label}>
          <div className="name">
            <span className="chev">{isOpen ? '▾' : '▸'}</span>
            <b>{g.label}</b>
            <span className="eco">
              {g.items.length} item{g.items.length === 1 ? '' : 's'}
              {sel !== g.items.length && ` · ${sel} selected`}
            </span>
          </div>
          <div className="ecos">
            {g.ecos.slice(0, 4).map(([e, b]) => (
              <span key={e}>
                {e} <small>{bytes(b)}</small>
              </span>
            ))}
            {g.ecos.length > 4 && <span>+{g.ecos.length - 4} more</span>}
          </div>
        </div>
        {/* Share of everything shown: where the weight is, at a glance, at any depth. */}
        <span className="share" title={`${((g.bytes / (visibleBytes || 1)) * 100).toFixed(1)}% of shown`}>
          <i style={{ width: `${(g.bytes / (visibleBytes || 1)) * 100}%` }} />
        </span>
        <span className="r size">{bytes(g.bytes)}</span>
        <span />
      </div>
    );
  };

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
        <button
          className={`cat-toggle ${grouped ? 'on' : ''}`}
          disabled={scanning || items.length === 0}
          onClick={() => setGroupBy((g) => (g ? null : 'folder'))}
          title={scanning ? 'Available when the scan finishes' : 'Group results into categories'}
          aria-pressed={grouped}
        >
          ▦ Categorize
        </button>
      </div>

      {groups && (
        <div className="group-bar">
          <div className="modes" role="group" aria-label="Group by">
            {GROUP_MODES.map((m) => (
              <button key={m.id} className={groupBy === m.id ? 'on' : ''} onClick={() => setGroupBy(m.id)}>
                {m.label}
              </button>
            ))}
          </div>
          <span className="dim">
            {groups.groups.length} categor{groups.groups.length === 1 ? 'y' : 'ies'}
            {groups.base && (
              <>
                {' '}under <b>{groups.base}</b>
              </>
            )}
          </span>
          <button
            className="link"
            onClick={() => setOpenKeys(() => (allOpen ? NO_OPEN : new Set(allKeys)))}
          >
            {allOpen ? 'Collapse all' : 'Expand all'}
          </button>
        </div>
      )}

      <div className="list-head">
        <input
          type="checkbox"
          checked={allChecked}
          onChange={(e) => setMany(visible, e.target.checked)}
          aria-label="Select all visible"
        />
        <span>{groups ? 'Category' : 'Folder'}</span>
        <span className="r">{groups ? '' : 'Last touched'}</span>
        <span className="r">Size</span>
      </div>

      <VirtualList
        items={rows}
        rowHeight={ROW_H}
        empty={
          scanning ? 'Looking…' : progress.status === 'idle' ? 'Pick drives or folders above, then Scan.' : 'Nothing matches.'
        }
        renderRow={(row, top) =>
          row.group
            ? renderGroup(row.group, top, row.depth)
            : row.item
              ? renderItem(row.item, top, row.depth)
              : renderItem(row, top, 0)
        }
      />

      <footer className="bar">
        <span className="dim">
          {visible.length} shown · {toDelete.length} selected
        </span>
        <button
          className="btn danger"
          disabled={toDelete.length === 0 || scanning || busy}
          onClick={() => onDelete(toDelete)}
        >
          {busy ? 'Deleting…' : `Delete ${bytes(toDeleteBytes)}`}
        </button>
      </footer>
    </>
  );
}
