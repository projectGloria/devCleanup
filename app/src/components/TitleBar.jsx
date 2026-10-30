import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

// Segoe Fluent Icons (Win11) / MDL2 Assets (Win10) codepoints: the same glyphs the
// native caption buttons use, so no icon assets ship with the app.
const GLYPH = { min: '\u{E921}', max: '\u{E922}', restore: '\u{E923}', close: '\u{E8BB}' };

/**
 * Replaces the native caption (window has `decorations: false`).
 * Dragging + double-click-to-maximize come from Tauri's built-in handler for
 * `data-tauri-drag-region`; it only fires when the attribute is on the element under
 * the cursor, so decorative text inside is made click-through in CSS.
 */
export function TitleBar({ children }) {
  const [maximized, setMaximized] = useState(false);

  // Window resize events are an external system; keep the restore/maximize glyph in sync.
  useEffect(() => {
    const win = getCurrentWindow();
    let alive = true;
    let unlisten;
    const sync = () => win.isMaximized().then((m) => alive && setMaximized(m));
    sync();
    // `onResized` resolves asynchronously: if StrictMode's dev double-mount (or a fast
    // unmount) runs cleanup first, unlisten as soon as the promise lands instead of leaking.
    win.onResized(sync).then((fn) => (alive ? (unlisten = fn) : fn()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const win = () => getCurrentWindow();
  return (
    <header className="titlebar" data-tauri-drag-region>
      {children}
      <div className="caption">
        <button onClick={() => win().minimize()} aria-label="Minimize">{GLYPH.min}</button>
        <button onClick={() => win().toggleMaximize()} aria-label={maximized ? 'Restore' : 'Maximize'}>
          {maximized ? GLYPH.restore : GLYPH.max}
        </button>
        <button className="close" onClick={() => win().close()} aria-label="Close">{GLYPH.close}</button>
      </div>
    </header>
  );
}
