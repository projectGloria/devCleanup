import { useEffect, useRef } from 'react';
import { bytes } from '../lib/format.js';

/** Native <dialog>: focus trap, Esc-to-close and backdrop for free. */
export function ConfirmDialog({ items, onConfirm, onClose }) {
  const ref = useRef(null);

  // showModal() is an imperative DOM API; syncing it with mount is what effects are for.
  // The `open` guard matters: StrictMode re-runs this effect in dev, and showModal()
  // on an already-open dialog throws InvalidStateError.
  useEffect(() => {
    const el = ref.current;
    if (el && !el.open) el.showModal();
  }, []);

  const total = items.reduce((n, it) => n + it.bytes, 0);
  const byTier = items.reduce((acc, it) => ({ ...acc, [it.tier]: (acc[it.tier] ?? 0) + 1 }), {});
  const top = [...items].sort((a, b) => b.bytes - a.bytes).slice(0, 6);

  return (
    <dialog ref={ref} className="confirm" onClose={onClose}>
      <h2>
        Delete {items.length} folder{items.length > 1 ? 's' : ''} · {bytes(total)}
      </h2>
      <p className="dim">
        {Object.entries(byTier)
          .map(([t, n]) => `${n} ${t}`)
          .join(' · ')}{' '}
        — permanent, not sent to the Recycle Bin. Each one is re-creatable with its restore command.
      </p>
      <ul>
        {top.map((it) => (
          <li key={it.id}>
            <span className="size">{bytes(it.bytes)}</span>
            <span className="path" title={it.path}>{it.path}</span>
            <code>{it.restore}</code>
          </li>
        ))}
        {items.length > top.length && <li className="dim">…and {items.length - top.length} more</li>}
      </ul>
      <div className="actions">
        <button className="btn ghost" onClick={() => ref.current.close()} autoFocus>
          Cancel
        </button>
        <button className="btn danger" onClick={onConfirm}>
          Delete permanently
        </button>
      </div>
    </dialog>
  );
}
