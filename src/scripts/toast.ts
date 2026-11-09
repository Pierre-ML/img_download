export type ToastKind = 'ok' | 'err' | 'info';

export function toast(message: string, kind: ToastKind = 'info', ms = 4000): void {
  const host = document.getElementById('toasts');
  if (!host) return;
  const el = document.createElement('div');
  const color =
    kind === 'ok' ? 'border-ok/50 text-ok' : kind === 'err' ? 'border-err/50 text-err' : 'border-accent/50 text-ink';
  el.className = `toast pointer-events-auto max-w-sm rounded-lg border bg-surface-2 px-4 py-2.5 text-sm shadow-xl ${color}`;
  el.setAttribute('role', 'status');
  el.textContent = message;
  host.appendChild(el);
  setTimeout(() => {
    el.style.transition = 'opacity .2s';
    el.style.opacity = '0';
    setTimeout(() => el.remove(), 220);
  }, ms);
}
