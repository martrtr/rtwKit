import { useEffect, useRef, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react';
export function Button({ className = '', type = 'button', ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...props} type={type} className={`rtw-button ${className}`} />;
}
const paths = {
  settings: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2',
  plus: 'M12 5v14M5 12h14',
  close: 'm6 6 12 12M18 6 6 18',
  arrow: 'M5 12h14m-6-6 6 6-6 6',
  chevron: 'm8 10 4 4 4-4',
  chat: 'M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2h-9l-5 3v-3H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Z',
  hash: 'm10 3-4 18M18 3l-4 18M3 9h18M2 15h18',
  search: 'M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z',
  refresh: 'M20 7v5h-5M4 17v-5h5M5.1 7a8 8 0 0 1 13.2-2L20 7M4 17l1.7 2A8 8 0 0 0 18.9 17',
  power: 'M12 2v10M6 5a9 9 0 1 0 12 0',
  grid: 'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',
  more: 'M5 11v2M12 11v2M19 11v2',
  logout: 'M9 4H4v16h5M9 12h12m-5-5 5 5-5 5',
  send: 'm3 3 18 9-18 9 4-9-4-9Zm4 9h14',
  world: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z',
  user: 'M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0ZM4 21v-2a8 8 0 0 1 16 0v2',
  split: 'M3 4h18v16H3zM12 4v16',
};
export function Icon({ name, size = 20 }: { name: keyof typeof paths; size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
export function Dialog({ open, title, onClose, children }: { open: boolean; title: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { if (open && !ref.current?.open) ref.current?.showModal(); else if (!open) ref.current?.close(); }, [open]);
  return <dialog className="rtw-dialog" ref={ref} onCancel={event => { event.preventDefault(); onClose(); }} onClick={event => { if (event.target === event.currentTarget) onClose(); }}><header><h2>{title}</h2><button className="icon-button" aria-label="Close dialog" onClick={onClose}><Icon name="close" /></button></header>{children}</dialog>;
}
export function Input(props: InputHTMLAttributes<HTMLInputElement>) { return <input {...props} className="rtw-input" />; }
export function Status({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <p className={`rtw-status${error ? ' error' : ''}`} role={error ? 'alert' : 'status'}>{children}</p>;
}
