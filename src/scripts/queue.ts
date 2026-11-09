import { queueCancel, queueClearFinished, queueList, onJobUpdate, onJobLog, errMsg, type JobInfo, type JobLog, type JobStatus } from './api';
import { toast } from './toast';
import { state } from './state';

const MAX_LOG = 300;
const jobs = new Map<string, JobInfo>();
const cards = new Map<string, HTMLElement>();
const $ = (id: string) => document.getElementById(id)!;

const STATUS: Record<JobStatus, { label: string; cls: string }> = {
  queued: { label: 'En attente', cls: 'bg-fg/5 text-muted' },
  running: { label: 'En cours', cls: 'bg-fg/10 text-fg' },
  done: { label: 'Terminé', cls: 'bg-success/15 text-success' },
  error: { label: 'Erreur', cls: 'bg-danger/15 text-danger' },
  cancelled: { label: 'Annulé', cls: 'bg-warning/15 text-warning' },
};

const stat = (label: string, value: number, cls = 'text-fg') =>
  `<div class="flex flex-col"><span class="text-[10px] uppercase tracking-wide text-muted">${label}</span><span class="font-medium tabular-nums ${cls}">${value}</span></div>`;

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);
}

function renderCard(j: JobInfo): void {
  let el = cards.get(j.id);
  if (!el) {
    el = document.createElement('div');
    el.className = 'panel flex flex-col gap-2.5 !p-3.5';
    cards.set(j.id, el);
    $('queue-list').appendChild(el);
  }
  const st = STATUS[j.status];
  const active = j.status === 'queued' || j.status === 'running';
  const determinate = j.limit > 0;
  const pct = determinate ? Math.min(100, Math.round((j.saved / j.limit) * 100)) : 0;
  const bar = determinate
    ? `<div class="h-full rounded-full bg-fg transition-[width] duration-200" style="width:${pct}%"></div>`
    : j.status === 'running'
      ? '<div class="animate-indeterminate absolute inset-y-0 w-2/5 rounded-full bg-fg"></div>'
      : `<div class="h-full rounded-full ${j.status === 'done' ? 'bg-success' : 'bg-border'} w-full"></div>`;
  const progressText = determinate ? `${j.saved} / ${j.limit}` : `${j.saved}`;
  el.dataset.id = j.id;
  el.innerHTML = `
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0 flex-1">
        <div class="truncate font-medium" title="${esc(j.label)}">${esc(j.label)}</div>
        <div class="mt-1 flex items-center gap-2">
          <span class="pill ${st.cls}">${st.label}</span>
          <span class="text-xs text-muted tabular-nums">${progressText}</span>
        </div>
      </div>
      ${active ? `<button data-cancel="${esc(j.id)}" class="rounded-md p-1 text-muted transition-colors hover:bg-danger/10 hover:text-danger" title="Annuler" aria-label="Annuler">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg></button>` : ''}
    </div>
    <div class="relative h-1.5 overflow-hidden rounded-full bg-fg/10">${bar}</div>
    <div class="grid grid-cols-4 gap-2 text-xs">
      ${stat('Trouvés', j.found)}${stat('Enreg.', j.saved, 'text-success')}${stat('Ignorés', j.skipped, 'text-muted')}${stat('Échecs', j.failed, j.failed ? 'text-danger' : 'text-muted')}
    </div>
    ${j.error ? `<div class="rounded-md bg-danger/10 px-2 py-1 text-xs text-danger">${esc(j.error)}</div>` : ''}`;
  el.querySelector<HTMLButtonElement>('[data-cancel]')?.addEventListener('click', async () => {
    try {
      await queueCancel(j.id);
    } catch (e) {
      toast(errMsg(e), 'err');
    }
  });
}

function renderAll(): void {
  // Supprime les cartes dont le job n'existe plus
  for (const [id, el] of cards) {
    if (!jobs.has(id)) {
      el.remove();
      cards.delete(id);
    }
  }
  for (const j of jobs.values()) renderCard(j);
  const finished = [...jobs.values()].some((j) => !['queued', 'running'].includes(j.status));
  ($('queue-clear') as HTMLButtonElement).disabled = !finished;
  $('queue-empty').hidden = jobs.size > 0;
}

let lastStatus = new Map<string, JobStatus>();

function onUpdate(j: JobInfo): void {
  const prev = lastStatus.get(j.id);
  lastStatus.set(j.id, j.status);
  jobs.set(j.id, j);
  renderCard(j);
  renderAll();
  if (prev && prev !== j.status) {
    if (j.status === 'done') toast(`${j.label} : ${j.saved} média(s) enregistré(s)`, 'ok');
    else if (j.status === 'error') toast(`${j.label} : ${j.error ?? 'erreur'}`, 'err', 7000);
  }
}

const LEVEL_CLS: Record<JobLog['level'], string> = { info: 'text-muted', warn: 'text-warning', error: 'text-danger' };

function addLog(l: JobLog): void {
  const box = $('log-list');
  const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 24;
  const row = document.createElement('div');
  row.className = `${LEVEL_CLS[l.level] ?? 'text-muted'} break-words`;
  const t = new Date(l.ts).toLocaleTimeString('fr-FR');
  const ts = document.createElement('span');
  ts.className = 'mr-2 text-muted/60';
  ts.textContent = t;
  row.append(ts, document.createTextNode(l.message));
  box.appendChild(row);
  while (box.childElementCount > MAX_LOG) box.firstElementChild!.remove();
  if (atBottom) box.scrollTop = box.scrollHeight;
}

export async function initQueue(): Promise<void> {
  $('queue-clear').addEventListener('click', async () => {
    try {
      await queueClearFinished();
      const list = await queueList();
      jobs.clear();
      lastStatus = new Map();
      list.forEach((j) => {
        jobs.set(j.id, j);
        lastStatus.set(j.id, j.status);
      });
      renderAll();
    } catch (e) {
      toast(errMsg(e), 'err');
    }
  });
  $('log-clear').addEventListener('click', () => ($('log-list').textContent = ''));
  renderAll();
  if (!state.tauri) return;
  await onJobUpdate(onUpdate);
  await onJobLog(addLog);
  try {
    const list = await queueList();
    list.forEach((j) => {
      jobs.set(j.id, j);
      lastStatus.set(j.id, j.status);
    });
    renderAll();
  } catch (e) {
    toast(errMsg(e), 'err');
  }
}
