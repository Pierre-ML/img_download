import { convertFileSrc } from '@tauri-apps/api/core';
import { recentMedia, onMediaSaved, errMsg, type MediaSaved } from './api';
import { toast } from './toast';
import { state } from './state';

const MAX_TILES = 1000;
const grid = () => document.getElementById('gallery-grid')!;

/**
 * Chargement paresseux + déchargement hors écran : seules les vignettes proches
 * du viewport gardent leur src, le reste est vidé pour tenir à 250+ éléments.
 */
let observer: IntersectionObserver | null = null;

function load(tile: HTMLElement): void {
  const media = tile.querySelector<HTMLImageElement | HTMLVideoElement>('[data-media]');
  if (!media || media.getAttribute('src')) return;
  media.setAttribute('src', tile.dataset.src!);
}

function unload(tile: HTMLElement): void {
  const media = tile.querySelector<HTMLImageElement | HTMLVideoElement>('[data-media]');
  if (!media || !media.getAttribute('src')) return;
  media.removeAttribute('src');
  if (media instanceof HTMLVideoElement) media.load();
}

function makeTile(m: MediaSaved): HTMLElement {
  const tile = document.createElement('button');
  tile.type = 'button';
  tile.className =
    'group relative aspect-square overflow-hidden rounded-lg border border-line bg-surface-2 transition-colors hover:border-accent';
  const name = m.path.split(/[\\/]/).pop() ?? m.path;
  tile.title = name;
  tile.dataset.path = m.path;
  tile.dataset.src = convertFileSrc(m.path) + (m.kind === 'video' ? '#t=0.1' : '');

  let media: HTMLElement;
  if (m.kind === 'video') {
    const v = document.createElement('video');
    v.muted = true;
    v.preload = 'metadata';
    v.playsInline = true;
    media = v;
  } else {
    const img = document.createElement('img');
    img.loading = 'lazy';
    img.decoding = 'async';
    img.alt = name;
    media = img;
  }
  media.dataset.media = '';
  media.className = 'h-full w-full object-cover';
  tile.appendChild(media);

  if (m.kind === 'video') {
    const play = document.createElement('span');
    play.className = 'pointer-events-none absolute inset-0 flex items-center justify-center';
    play.innerHTML =
      '<span class="flex h-9 w-9 items-center justify-center rounded-full bg-black/60 text-white backdrop-blur-sm"><svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg></span>';
    tile.appendChild(play);
  }

  const dims = m.width && m.height ? `${m.width}×${m.height}` : m.kind === 'video' ? 'MP4' : '';
  if (dims) {
    const b = document.createElement('span');
    b.className =
      'pointer-events-none absolute bottom-1 left-1 rounded bg-black/70 px-1.5 py-0.5 text-[10px] tabular-nums text-ink opacity-0 transition-opacity group-hover:opacity-100';
    b.textContent = dims;
    tile.appendChild(b);
  }

  tile.addEventListener('click', async () => {
    try {
      const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
      await revealItemInDir(m.path);
    } catch (e) {
      toast(errMsg(e), 'err');
    }
  });
  observer?.observe(tile);
  return tile;
}

function updateCount(): void {
  const n = grid().childElementCount;
  document.getElementById('gallery-count')!.textContent = n ? String(n) : '';
  document.getElementById('gallery-empty')!.hidden = n > 0;
}

function prepend(m: MediaSaved): void {
  const g = grid();
  g.prepend(makeTile(m));
  while (g.childElementCount > MAX_TILES) {
    const last = g.lastElementChild as HTMLElement;
    observer?.unobserve(last);
    last.remove();
  }
  updateCount();
}

export async function initGallery(): Promise<void> {
  observer = new IntersectionObserver(
    (entries) => {
      for (const e of entries) (e.isIntersecting ? load : unload)(e.target as HTMLElement);
    },
    { root: document.getElementById('gallery-scroll'), rootMargin: '300px 0px' },
  );
  updateCount();
  if (!state.tauri) return;
  await onMediaSaved(prepend);
  try {
    const list = await recentMedia(200);
    const frag = document.createDocumentFragment();
    // plus récent d'abord, on garde l'ordre ; les médias arrivés entre-temps restent en tête
    list.forEach((m) => frag.appendChild(makeTile(m)));
    grid().appendChild(frag);
    updateCount();
  } catch (e) {
    toast(errMsg(e), 'err');
  }
}
