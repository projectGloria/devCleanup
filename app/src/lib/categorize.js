// Groups scan results for the optional "Categorize" view. Pure functions: the view
// derives groups from (visible items, mode) during render, memoized.

export const GROUP_MODES = [
  { id: 'folder', label: 'Folders' },
  { id: 'ecosystem', label: 'Ecosystem' },
  { id: 'kind', label: 'Type' },
];

const KIND_LABEL = { deps: 'Dependencies', build: 'Build output', cache: 'Caches' };

const split = (p) => p.split(/[\\/]+/).filter(Boolean);
const node = (label) => ({ label, kids: new Map(), items: [] });

/**
 * The "smart" part: walk down while a folder has nothing of its own and exactly one
 * sub-folder. `C:\ › Users › Nihil › Desktop › Repos` with every result under Repos
 * collapses to one hop, so groups start where the tree actually branches.
 */
function squash(n) {
  const parts = [n.label];
  while (n.items.length === 0 && n.kids.size === 1) {
    n = n.kids.values().next().value;
    parts.push(n.label);
  }
  return { node: n, parts };
}

function collect(n, out = []) {
  out.push(...n.items);
  for (const k of n.kids.values()) collect(k, out);
  return out;
}

const join = (parts) => {
  const p = parts.filter(Boolean).join('\\');
  return /^[a-z]:$/i.test(p) ? `${p}\\` : p; // a bare "C:" means the drive root
};

function summarize(key, label, hint, items) {
  const eco = new Map();
  let total = 0;
  for (const it of items) {
    total += it.bytes;
    eco.set(it.ecosystem, (eco.get(it.ecosystem) ?? 0) + it.bytes);
  }
  return { key, label, hint, items, bytes: total, ecos: [...eco].sort((a, b) => b[1] - a[1]) };
}

/**
 * Folder tree of every project path, grouped at the first real branch point.
 * No scan roots needed: squashing from the drive letter down walks through the root
 * (and past it, when everything sits in one sub-folder) on its own.
 */
function byFolder(items) {
  const top = node('');
  for (const it of items) {
    let n = top;
    for (const seg of split(it.project)) {
      const k = seg.toLowerCase(); // NTFS is case-insensitive
      let c = n.kids.get(k);
      if (!c) n.kids.set(k, (c = node(seg)));
      n = c;
    }
    n.items.push(it);
  }

  const { node: anchor, parts } = squash(top);
  const root = build(anchor, parts, []);
  return { base: root.hint, groups: root.children, own: root.own };
}

const bySize = (a, b) => b.bytes - a.bytes;

/**
 * Recursive group: every sub-folder becomes a child group (itself squashed, so
 * `work\clients\acme` is one hop when nothing branches in between) and results living
 * directly in this folder are its `own` rows. `items` = all descendants, which is what
 * the group checkbox selects.
 */
function build(n, parts, prefix) {
  const path = [...prefix, ...parts];
  const hint = join(path);
  const children = [...n.kids.values()]
    .map((k) => {
      const s = squash(k);
      return build(s.node, s.parts, path);
    })
    .sort(bySize);
  return {
    ...summarize(hint.toLowerCase(), parts.join('\\'), hint, collect(n).sort(bySize)),
    own: [...n.items].sort(bySize),
    children,
  };
}

function byKey(items, keyOf, labelOf) {
  const m = new Map();
  for (const it of items) {
    const k = keyOf(it);
    if (!m.has(k)) m.set(k, []);
    m.get(k).push(it);
  }
  const groups = [...m].map(([k, list]) => ({ ...summarize(k, labelOf(k), '', list), own: list, children: [] }));
  return { base: '', groups: groups.sort(bySize), own: [] };
}

export function categorize(items, mode) {
  if (mode === 'ecosystem') return byKey(items, (i) => i.ecosystem, (k) => k);
  if (mode === 'kind') return byKey(items, (i) => i.kind, (k) => KIND_LABEL[k] ?? k);
  return byFolder(items);
}
