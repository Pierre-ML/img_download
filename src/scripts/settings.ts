import { getSettings, saveSettings, errMsg, type Settings } from './api';
import { toast } from './toast';
import { state } from './state';

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let current: Settings | null = null;

function fill(s: Settings): void {
  current = s;
  $<HTMLInputElement>('set-dest').value = s.destDir;
  $<HTMLInputElement>('set-parallel').value = String(s.parallel);
  $<HTMLInputElement>('set-png').checked = s.optimizePng;
  $<HTMLInputElement>('set-reencode').checked = s.reencodeVideo;
  $<HTMLInputElement>('set-crf').value = String(s.videoCrf);
  $('set-crf-val').textContent = String(s.videoCrf);
  $<HTMLInputElement>('set-crf').disabled = !s.reencodeVideo;
}

function read(): Settings {
  const clamp = (v: number, a: number, b: number, d: number) => (Number.isFinite(v) ? Math.min(b, Math.max(a, Math.round(v))) : d);
  return {
    destDir: $<HTMLInputElement>('set-dest').value.trim(),
    parallel: clamp(parseInt($<HTMLInputElement>('set-parallel').value, 10), 1, 16, 4),
    optimizePng: $<HTMLInputElement>('set-png').checked,
    reencodeVideo: $<HTMLInputElement>('set-reencode').checked,
    videoCrf: clamp(parseInt($<HTMLInputElement>('set-crf').value, 10), 18, 35, 23),
  };
}

async function save(): Promise<void> {
  const btn = $<HTMLButtonElement>('set-save');
  btn.disabled = true;
  try {
    fill(await saveSettings(read()));
    toast('Réglages enregistrés', 'ok');
  } catch (e) {
    toast(errMsg(e), 'err');
  } finally {
    btn.disabled = !state.tauri;
  }
}

export async function initSettings(): Promise<void> {
  $<HTMLInputElement>('set-crf').addEventListener('input', (e) => {
    $('set-crf-val').textContent = (e.target as HTMLInputElement).value;
  });
  $<HTMLInputElement>('set-reencode').addEventListener('change', (e) => {
    $<HTMLInputElement>('set-crf').disabled = !(e.target as HTMLInputElement).checked;
  });
  $('set-save').addEventListener('click', () => void save());

  $('set-choose').addEventListener('click', async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const dir = await open({ directory: true, multiple: false, defaultPath: $<HTMLInputElement>('set-dest').value || undefined });
      if (typeof dir === 'string') $<HTMLInputElement>('set-dest').value = dir;
    } catch (e) {
      toast(errMsg(e), 'err');
    }
  });

  $('set-open').addEventListener('click', async () => {
    const dir = $<HTMLInputElement>('set-dest').value.trim() || current?.destDir;
    if (!dir) return;
    try {
      const { openPath } = await import('@tauri-apps/plugin-opener');
      await openPath(dir);
    } catch (e) {
      toast(errMsg(e), 'err');
    }
  });

  for (const id of ['set-choose', 'set-open', 'set-save']) $<HTMLButtonElement>(id).disabled = !state.tauri;
  if (!state.tauri) return;
  try {
    fill(await getSettings());
  } catch (e) {
    toast(errMsg(e), 'err');
  }
}
