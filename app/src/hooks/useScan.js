import { useCallback, useRef, useState, startTransition } from 'react';
import { invoke, Channel } from '@tauri-apps/api/core';

const IDLE = { status: 'idle', dirs: 0, found: 0, bytes: 0, ms: 0, errors: 0 };

/**
 * Streams scan results from Rust over a Tauri Channel.
 *
 * Closure safety: a Channel's onmessage closure lives as long as the Rust thread
 * holding it. If the user restarts a scan, the OLD scan's flusher may still deliver
 * a final batch. Each scan captures its own token and compares it with `scanRef`,
 * so late messages from a superseded scan are dropped instead of polluting state.
 */
export function useScan() {
  const [items, setItems] = useState([]);
  const [progress, setProgress] = useState(IDLE);
  // Ref = source of truth for the message guard (read inside long-lived closures);
  // state mirror = what render-time code (selection stamps) keys on.
  const scanRef = useRef(0);
  const [scanId, setScanId] = useState(0);

  const start = useCallback((roots) => {
    const token = ++scanRef.current;
    setScanId(token);
    setItems([]);
    setProgress({ ...IDLE, status: 'scanning' });

    const ch = new Channel();
    ch.onmessage = (ev) => {
      if (token !== scanRef.current) return;
      switch (ev.type) {
        case 'batch':
          // Transition: appending thousands of rows is non-urgent; typing in the
          // search box or clicking Cancel must stay responsive while it renders.
          startTransition(() => setItems((prev) => prev.concat(ev.items)));
          break;
        case 'progress':
          setProgress((p) => ({ ...p, dirs: ev.dirs, found: ev.found, bytes: ev.bytes }));
          break;
        case 'done':
          setProgress((p) => ({
            ...p,
            status: ev.cancelled ? 'cancelled' : 'done',
            dirs: ev.dirs,
            errors: ev.errors,
            ms: ev.ms,
          }));
          break;
      }
    };
    invoke('start_scan', { roots, onEvent: ch }).catch((e) => {
      setProgress({ ...IDLE, status: 'error', error: String(e) });
    });
  }, []);

  const cancel = useCallback(() => invoke('cancel_scan'), []);

  // Functional update: never close over a stale `items` array.
  const removeIds = useCallback((ids) => {
    const gone = new Set(ids);
    setItems((prev) => prev.filter((it) => !gone.has(it.id)));
  }, []);

  return { items, scanId, progress, start, cancel, removeIds };
}
