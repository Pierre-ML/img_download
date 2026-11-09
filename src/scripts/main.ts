import { inTauri, ffmpegStatus } from './api';
import { state } from './state';
import { initNav } from './nav';
import { initSources } from './sources';
import { initAccounts } from './accounts';
import { initSettings } from './settings';
import { initQueue } from './queue';
import { initGallery } from './gallery';
import { initUpdate } from './update';
import { initResize } from './resize';

async function initFfmpeg(): Promise<void> {
  const dot = document.getElementById('ffmpeg-dot')!;
  const text = document.getElementById('ffmpeg-text')!;
  if (!state.tauri) {
    text.textContent = 'ffmpeg : indisponible';
    return;
  }
  try {
    const s = await ffmpegStatus();
    if (s.available) {
      dot.className = 'size-2 rounded-full bg-success';
      text.textContent = `ffmpeg ${s.version ?? ''}`.trim();
      text.title = s.path ?? '';
    } else {
      dot.className = 'size-2 rounded-full bg-danger';
      text.textContent = 'ffmpeg introuvable';
    }
  } catch {
    dot.className = 'size-2 rounded-full bg-danger';
    text.textContent = 'ffmpeg : erreur';
  }
}

async function boot(): Promise<void> {
  state.tauri = inTauri();
  document.getElementById('browser-banner')!.hidden = state.tauri;
  initResize();
  initNav();
  initSources();
  initUpdate();
  await Promise.allSettled([initAccounts(), initSettings(), initQueue(), initGallery(), initFfmpeg()]);
}

void boot();
