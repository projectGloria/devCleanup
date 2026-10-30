import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

/**
 * Drive list + free space, re-read on demand.
 *
 * The old version fetched once on mount, so free space was a snapshot from app start.
 * Now callers `refresh()` whenever space can change (during/after deletes, after cache
 * cleans), and the window's focus event catches changes made outside the app.
 * `baseline` = free bytes per drive at first read, so the UI can show "+X freed".
 */
export function useDrives() {
  const [state, setState] = useState({ list: [], baseline: null });

  const refresh = useCallback(
    () =>
      invoke('list_drives')
        .then((list) =>
          // First successful read becomes the baseline; later reads keep it.
          setState((s) => ({ list, baseline: s.baseline ?? new Map(list.map((d) => [d.path, d.free])) })),
        )
        .catch(() => {}),
    [],
  );

  // Subscribing to window focus is a real external system: effect is correct here.
  useEffect(() => {
    refresh();
    window.addEventListener('focus', refresh);
    return () => window.removeEventListener('focus', refresh);
  }, [refresh]);

  return { drives: state.list, baseline: state.baseline, refresh };
}
