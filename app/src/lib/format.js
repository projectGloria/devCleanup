const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function bytes(n) {
  let v = n;
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${n} B` : `${v.toFixed(v >= 100 ? 0 : v >= 10 ? 1 : 2)} ${UNITS[u]}`;
}

const DAY = 86_400;

export function age(unixSecs, nowSecs) {
  if (!unixSecs) return '—';
  const d = Math.floor((nowSecs - unixSecs) / DAY);
  if (d < 1) return 'today';
  if (d < 30) return `${d}d ago`;
  if (d < 365) return `${Math.floor(d / 30)}mo ago`;
  return `${(d / 365).toFixed(1)}y ago`;
}

export const nowSecs = () => Math.floor(Date.now() / 1000);
export const DAY_SECS = DAY;
