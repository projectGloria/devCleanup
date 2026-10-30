import { useCallback, useState } from 'react';
import { invoke, Channel } from '@tauri-apps/api/core';

/**
 * Drives one `delete_items` run and keeps a live model of it for the progress panel.
 *
 * Job shape:
 *   phase    'staging' -> 'purging' -> 'done'
 *   byId     Map<id, candidate>   — a private copy: rows leave the scan list once
 *                                   staged, but the panel still needs names/sizes
 *   total*   expected totals, from scan-time sizing
 *   freed/files/ms                — live counters from the backend ticker (100 ms)
 *   active   [{id, freed, files, current}] — folders being emptied right now
 *   finished [{id, freed, files, errors}]  — newest first
 */
export function useDelete() {
  const [job, setJob] = useState(null);

  // `hooks` are passed per call (not captured at hook creation) so a long-lived Channel
  // closure always calls the handlers that were current when THIS delete started.
  const start = useCallback((list, hooks) => {
    const byId = new Map(list.map((i) => [i.id, i]));
    setJob({
      phase: 'staging',
      byId,
      total: list.length,
      totalBytes: list.reduce((n, i) => n + i.bytes, 0),
      totalFiles: list.reduce((n, i) => n + i.files, 0),
      freed: 0,
      files: 0,
      ms: 0,
      active: [],
      finished: [],
      failed: [],
      errors: [],
    });

    const ch = new Channel();
    ch.onmessage = (ev) => {
      switch (ev.type) {
        case 'staged':
          // Folders are already renamed away on disk: drop their rows right now.
          hooks.onStaged(ev.ids);
          setJob((j) => ({ ...j, phase: 'purging', total: ev.ids.length, failed: ev.failed }));
          break;
        case 'progress':
          // A tick sampled just before the last item finished can land after `done`.
          setJob((j) =>
            j.phase === 'done' ? j : { ...j, freed: ev.freed, files: ev.files, ms: ev.ms, active: ev.active },
          );
          break;
        case 'item':
          // `active` is left alone: the next tick (<=100 ms) replaces it wholesale.
          // Filtering here could empty the section between ticks and make the layout jump.
          setJob((j) => ({ ...j, finished: [ev, ...j.finished] }));
          break;
        case 'done':
          setJob((j) => ({ ...j, phase: 'done', freed: ev.freed, files: ev.files, ms: ev.ms, errors: ev.errors, active: [] }));
          hooks.onDone(ev);
          break;
      }
    };
    invoke('delete_items', { ids: list.map((i) => i.id), onEvent: ch });
  }, []);

  const clear = useCallback(() => setJob(null), []);
  return { job, start, clear };
}
