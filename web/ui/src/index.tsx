import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from 'react';
export function Button({ className = '', ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...props} className={`rtw-button ${className}`} />;
}
export function Input(props: InputHTMLAttributes<HTMLInputElement>) { return <input {...props} className="rtw-input" />; }
export function Status({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <p className={`rtw-status${error ? ' error' : ''}`} role={error ? 'alert' : 'status'}>{children}</p>;
}
