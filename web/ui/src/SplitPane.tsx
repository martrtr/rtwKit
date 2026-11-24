import { useEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';

// Local layout only. The owning extension persists its ratio through its own preferences.
export function SplitPane({ sidebar, children, className = '', open, ratio, onResize, onCollapse, sidebarId, label }: {
  sidebar: ReactNode; children: ReactNode; className?: string; open: boolean;
  ratio: number; onResize: (ratio: number) => void; onCollapse: () => void;
  sidebarId: string; label: string;
}) {
  const root = useRef<HTMLElement>(null), handle = useRef<HTMLDivElement>(null);
  const drag = useRef<{ pointer: number; offset: number; value: number } | null>(null);
  const [width, setWidth] = useState(0), [draft, setDraft] = useState<number | null>(null);
  const preferred = Number.isFinite(ratio) ? Math.max(0, Math.min(1, ratio)) : .25;
  const available = Math.max(0, width - 8);
  const minimum = Math.min(160, available * .45);
  const maximum = Math.max(minimum, Math.min(420, available - Math.min(280, available * .55)));
  const clamp = (pixels: number) => Math.max(minimum, Math.min(maximum, pixels));
  const size = clamp((draft ?? preferred) * available);
  const cancel = () => {
    const active = drag.current; drag.current = null; setDraft(null);
    if (active && handle.current?.hasPointerCapture(active.pointer)) handle.current.releasePointerCapture(active.pointer);
  };
  useEffect(() => {
    const element = root.current!;
    const observer = new ResizeObserver(() => setWidth(element.getBoundingClientRect().width));
    setWidth(element.getBoundingClientRect().width); observer.observe(element);
    return () => { observer.disconnect(); const active = drag.current; drag.current = null; if (active && handle.current?.hasPointerCapture(active.pointer)) handle.current.releasePointerCapture(active.pointer); };
  }, []);
  useEffect(() => { if (!open) cancel(); }, [open]);
  const commit = (pixels: number) => { if (available > 0) onResize(clamp(pixels) / available); };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointer !== event.pointerId || available <= 0) return;
    const next = clamp(event.clientX - root.current!.getBoundingClientRect().left - drag.current.offset) / available;
    drag.current.value = next; setDraft(next);
  };
  const key = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape' && drag.current) { event.preventDefault(); cancel(); return; }
    if (event.key === 'Enter') { event.preventDefault(); cancel(); onCollapse(); return; }
    const step = event.shiftKey ? 48 : 16;
    const next = event.key === 'ArrowLeft' ? size - step : event.key === 'ArrowRight' ? size + step : event.key === 'Home' ? minimum : event.key === 'End' ? maximum : null;
    if (next !== null) { event.preventDefault(); cancel(); commit(next); }
  };
  return <main ref={root} className={`rtw-split ${className}`} data-open={open} style={{ '--rtw-pane-size': `${size}px` } as CSSProperties}>
    {sidebar}
    {open && <div ref={handle} className="rtw-pane-handle" role="separator" aria-label={label} aria-controls={sidebarId} aria-orientation="vertical" aria-valuemin={Math.round(minimum)} aria-valuemax={Math.round(maximum)} aria-valuenow={Math.round(size)} aria-valuetext={`${Math.round(size)} pixels`} aria-keyshortcuts="ArrowLeft ArrowRight Home End Enter" tabIndex={0}
      onKeyDown={key} onDoubleClick={() => { cancel(); onResize(.25); }}
      onPointerDown={event => { if (event.button !== 0 || drag.current) return; event.preventDefault(); event.currentTarget.focus(); event.currentTarget.setPointerCapture(event.pointerId); drag.current = { pointer: event.pointerId, offset: event.clientX - root.current!.getBoundingClientRect().left - size, value: size / Math.max(1, available) }; }}
      onPointerMove={move} onPointerUp={event => { if (drag.current?.pointer !== event.pointerId) return; move(event); const value = drag.current.value; cancel(); commit(value * available); }}
      onPointerCancel={cancel} onLostPointerCapture={() => { if (drag.current) cancel(); }} />}
    {children}
  </main>;
}
