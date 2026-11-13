import { getSettings, saveSettings, appUninstall, errMsg, type Settings, type ImageFormat, type VideoFormat } from './api';
import { toast } from './toast';
import { state } from './state';

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let current: Settings | null = null;

/** Réglages courants (null tant qu'ils ne sont pas chargés). */
export const currentSettings = (): Settings | null => current;

/** Active/désactive les contrôles selon les formats choisis. */
function syncFormatControls(): void {
  const img = $<HTMLSelectElement>('set-image-format').value;
  const vid = $<HTMLSelectElement>('set-video-format').value;
  const h264 = vid === 'h264';
  const reencode = $<HTMLInputElement>('set-reencode');
  $<HTMLInputElement>('set-image-quality').disabled = img === 'png';
  $<HTMLInputElement>('set-png').disabled = img !== 'png';
  reencode.disabled = !h264;
  $('set-reencode-help').textContent = h264
    ? 'Convertit en MP4 H.264 ; sinon la vidéo est simplement copiée si possible'
    : 'Ce format est toujours ré-encodé (option réservée au MP4 H.264)';
  $<HTMLInputElement>('set-crf').disabled = h264 && !reencode.checked;
}

function fill(s: Settings): void {
  current = s;
  $<HTMLSelectElement>('set-image-format').value = s.imageFormat;
  $<HTMLSelectElement>('set-video-format').value = s.videoFormat;
  $<HTMLInputElement>('set-image-quality').value = String(s.imageQuality);
  $('set-image-quality-val').textContent = String(s.imageQuality);
  $<HTMLInputElement>('set-dest').value = s.destDir;
  $<HTMLInputElement>('set-parallel').value = String(s.parallel);
  $<HTMLInputElement>('set-png').checked = s.optimizePng;
  $<HTMLInputElement>('set-reencode').checked = s.reencodeVideo;
  $<HTMLInputElement>('set-crf').value = String(s.videoCrf);
  $('set-crf-val').textContent = String(s.videoCrf);
  syncFormatControls();
  document.dispatchEvent(new CustomEvent<Settings>('settings-changed', { detail: s }));
}

function read(): Settings {
  const clamp = (v: number, a: number, b: number, d: number) => (Number.isFinite(v) ? Math.min(b, Math.max(a, Math.round(v))) : d);
  return {
    destDir: $<HTMLInputElement>('set-dest').value.trim(),
    parallel: clamp(parseInt($<HTMLInputElement>('set-parallel').value, 10), 1, 16, 4),
    optimizePng: $<HTMLInputElement>('set-png').checked,
    reencodeVideo: $<HTMLInputElement>('set-reencode').checked,
    videoCrf: clamp(parseInt($<HTMLInputElement>('set-crf').value, 10), 18, 35, 23),
    imageFormat: $<HTMLSelectElement>('set-image-format').value as ImageFormat,
    videoFormat: $<HTMLSelectElement>('set-video-format').value as VideoFormat,
    imageQuality: clamp(parseInt($<HTMLInputElement>('set-image-quality').value, 10), 50, 100, 85),
  };
}

async function uninstall(): Promise<void> {
  const btn = $<HTMLButtonElement>('app-uninstall');
  try {
    const { ask } = await import('@tauri-apps/plugin-dialog');
    const ok = await ask('Désinstaller Lucascrap ? L\'application va se fermer. Les médias téléchargés sont conservés.', {
      title: 'Désinstaller',
      kind: 'warning',
      okLabel: 'Désinstaller',
      cancelLabel: 'Annuler',
    });
    if (!ok) return;
    btn.disabled = true;
    await appUninstall();
  } catch (e) {
    toast(errMsg(e), 'err');
    btn.disabled = !state.tauri;
  }
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
  $<HTMLInputElement>('set-image-quality').addEventListener('input', (e) => {
    $('set-image-quality-val').textContent = (e.target as HTMLInputElement).value;
  });
  $<HTMLInputElement>('set-reencode').addEventListener('change', syncFormatControls);
  $<HTMLSelectElement>('set-image-format').addEventListener('change', syncFormatControls);
  $<HTMLSelectElement>('set-video-format').addEventListener('change', syncFormatControls);
  $('set-save').addEventListener('click', () => void save());
  $('app-uninstall').addEventListener('click', () => void uninstall());

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

  for (const id of ['set-choose', 'set-open', 'set-save', 'app-uninstall']) $<HTMLButtonElement>(id).disabled = !state.tauri;
  if (!state.tauri) return;
  try {
    fill(await getSettings());
  } catch (e) {
    toast(errMsg(e), 'err');
  }
}
