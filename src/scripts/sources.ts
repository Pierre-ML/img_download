import { queueAdd, errMsg, type JobRequest, type Settings, type ImageFormat, type VideoFormat } from './api';
import { currentSettings } from './settings';
import { toast } from './toast';
import { state } from './state';
import { showScreen } from './nav';

const IMAGE_LABELS: Record<ImageFormat, string> = { png: 'PNG', jpg: 'JPG', webp: 'WebP', avif: 'AVIF' };
const VIDEO_LABELS: Record<VideoFormat, string> = {
  h264: 'MP4 H.264',
  h265: 'MP4 H.265',
  vp9: 'WebM VP9',
  av1: 'WebM AV1',
};
const FORMAT_PREFIXES = ['google', 'x', 'reddit'];

function fillSelect(sel: HTMLSelectElement, labels: Record<string, string>, def: string): void {
  const prev = sel.value;
  sel.replaceChildren();
  const first = document.createElement('option');
  first.value = '';
  first.textContent = `Réglages (${labels[def] ?? def})`;
  sel.appendChild(first);
  for (const [value, label] of Object.entries(labels)) {
    const o = document.createElement('option');
    o.value = value;
    o.textContent = label;
    sel.appendChild(o);
  }
  sel.value = prev;
}

/** Met à jour le libellé « Réglages (…) » des sélecteurs de format. */
function syncFormatSelects(s: Settings | null): void {
  for (const p of FORMAT_PREFIXES) {
    fillSelect(document.getElementById(`${p}-image-format`) as HTMLSelectElement, IMAGE_LABELS, s?.imageFormat ?? 'png');
    fillSelect(document.getElementById(`${p}-video-format`) as HTMLSelectElement, VIDEO_LABELS, s?.videoFormat ?? 'h264');
  }
}

function formatsOf(prefix: string): { imageFormat?: ImageFormat; videoFormat?: VideoFormat } {
  const img = (document.getElementById(`${prefix}-image-format`) as HTMLSelectElement).value;
  const vid = (document.getElementById(`${prefix}-video-format`) as HTMLSelectElement).value;
  const out: { imageFormat?: ImageFormat; videoFormat?: VideoFormat } = {};
  if (img) out.imageFormat = img as ImageFormat;
  if (vid) out.videoFormat = vid as VideoFormat;
  return out;
}

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
    void add(gBtn, { kind: 'google', query: q, limit, ...formatsOf('google') });
  };
  query.addEventListener('input', refreshButtons);
  query.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') submitGoogle();
  });
  gBtn.addEventListener('click', submitGoogle);

  document
    .getElementById('x-add')!
    .addEventListener('click', (e) =>
      void add(e.currentTarget as HTMLButtonElement, { kind: 'xLikes', limit: limitOf('x-limit'), ...formatsOf('x') }),
    );
  document
    .getElementById('reddit-add')!
    .addEventListener('click', (e) =>
      void add(e.currentTarget as HTMLButtonElement, { kind: 'redditSaved', limit: limitOf('reddit-limit'), ...formatsOf('reddit') }),
    );
  for (const p of ['x', 'reddit']) {
    document.getElementById(`${p}-connect`)!.addEventListener('click', () => showScreen('accounts'));
  }
  syncFormatSelects(currentSettings());
  document.addEventListener('settings-changed', (e) => syncFormatSelects((e as CustomEvent<Settings>).detail));
  refreshButtons();
}
