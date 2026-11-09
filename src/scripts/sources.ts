import { queueAdd, errMsg, type JobRequest } from './api';
import { toast } from './toast';
import { state } from './state';
import { showScreen } from './nav';

function limitOf(id: string, fallback = 0): number {
  const v = parseInt((document.getElementById(id) as HTMLInputElement).value, 10);
  return Number.isFinite(v) && v >= 0 ? v : fallback;
}

async function add(btn: HTMLButtonElement, req: JobRequest): Promise<void> {
  btn.disabled = true;
  try {
    await queueAdd(req);
    toast("Ajouté à la file d'attente", 'ok');
  } catch (e) {
    toast(errMsg(e), 'err');
  } finally {
    refreshButtons();
  }
}

/** Active/désactive les boutons selon le backend et l'état des comptes. */
export function refreshButtons(): void {
  const g = document.getElementById('google-add') as HTMLButtonElement;
  const q = document.getElementById('google-query') as HTMLInputElement;
  g.disabled = !state.tauri || q.value.trim() === '';
  const x = document.getElementById('x-add') as HTMLButtonElement;
  x.disabled = !state.tauri || !state.accounts?.x.connected;
  const r = document.getElementById('reddit-add') as HTMLButtonElement;
  r.disabled = !state.tauri || !state.accounts?.reddit.connected;
}

export function renderSourceAccounts(): void {
  for (const p of ['x', 'reddit'] as const) {
    const info = state.accounts?.[p];
    const status = document.getElementById(`${p}-status`)!;
    const connect = document.getElementById(`${p}-connect`)!;
    if (info?.connected) {
      status.textContent = `Connecté${info.username ? ' en tant que @' + info.username : ''}`;
      status.className = 'text-sm text-success';
      connect.hidden = true;
    } else {
      status.textContent = 'Non connecté';
      status.className = 'text-sm text-muted';
      connect.hidden = false;
    }
  }
  refreshButtons();
}

export function initSources(): void {
  const query = document.getElementById('google-query') as HTMLInputElement;
  const gBtn = document.getElementById('google-add') as HTMLButtonElement;
  const submitGoogle = () => {
    const q = query.value.trim();
    if (!q || gBtn.disabled) return;
    const limit = Math.max(1, limitOf('google-limit', 50));
    void add(gBtn, { kind: 'google', query: q, limit });
  };
  query.addEventListener('input', refreshButtons);
  query.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') submitGoogle();
  });
  gBtn.addEventListener('click', submitGoogle);

  document
    .getElementById('x-add')!
    .addEventListener('click', (e) =>
      void add(e.currentTarget as HTMLButtonElement, { kind: 'xLikes', limit: limitOf('x-limit') }),
    );
  document
    .getElementById('reddit-add')!
    .addEventListener('click', (e) =>
      void add(e.currentTarget as HTMLButtonElement, { kind: 'redditSaved', limit: limitOf('reddit-limit') }),
    );
  for (const p of ['x', 'reddit']) {
    document.getElementById(`${p}-connect`)!.addEventListener('click', () => showScreen('accounts'));
  }
  refreshButtons();
}
