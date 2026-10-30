import { useLayoutEffect, useRef, useState } from 'react';

const OVERSCAN = 8;

/**
 * Fixed-row-height virtualizer (~30 lines instead of a dependency).
 * Only the rows in the viewport (+overscan) exist in the DOM, so 20k candidates
 * cost the same to render as 40. Rows are absolutely positioned inside a spacer
 * whose height equals the full list, which keeps the native scrollbar accurate.
 */
export function VirtualList({ items, rowHeight, renderRow, empty }) {
  const ref = useRef(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(600);

  // ResizeObserver is a real external system: an effect is the right tool here.
  useLayoutEffect(() => {
    const el = ref.current;
    const ro = new ResizeObserver(([entry]) => setHeight(entry.contentRect.height));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  const last = Math.min(items.length, Math.ceil((scrollTop + height) / rowHeight) + OVERSCAN);

  // The scroll container always mounts (even when empty) so the observer above
  // always has a node; swapping elements would orphan it.
  return (
    <div ref={ref} className="vlist" onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
      {items.length === 0 ? (
        <div className="empty">{empty}</div>
      ) : (
        <div style={{ height: items.length * rowHeight, position: 'relative' }}>
          {items.slice(first, last).map((it, i) => renderRow(it, (first + i) * rowHeight))}
        </div>
      )}
    </div>
  );
}
