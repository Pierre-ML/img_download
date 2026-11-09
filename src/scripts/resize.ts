// Zones redimensionnables à la souris (et au clavier), tailles mémorisées dans localStorage.

export interface LayoutState {
  sidebar: number; // px
  queue: number; // px
  queueSplit: number; // ratio 0..1 (part de la File d'attente)
  gallery: number; // ratio 0..1 (part de la hauteur de main)
}

export const LAYOUT_KEY = 'mdl.layout';
export const LAYOUT_DEFAULTS: LayoutState = { sidebar: 256, queue: 360, queueSplit: 0.6, gallery: 0.46 };

const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 420;
const QUEUE_MIN = 260;
const QUEUE_MAX = 720;
const MAIN_MIN_W = 360;
const SPLIT_MIN = 100;
const GALLERY_MIN = 120;
const SCREEN_MIN_H = 160;
const KEY_PX = 16;
const KEY_RATIO = 0.04;

let layout: LayoutState = { ...LAYOUT_DEFAULTS };

function load(): LayoutState {
  const out = { ...LAYOUT_DEFAULTS };
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return out;
    const o = JSON.parse(raw) as Partial<Record<keyof LayoutState, unknown>>;
    for (const k of Object.keys(out) as (keyof LayoutState)[]) {
      const v = o[k];
      if (typeof v === 'number' && Number.isFinite(v)) out[k] = v;
    }
  } catch {
    /* storage indisponible ou JSON invalide : défauts */
  }
  return out;
}

function save(): void {
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    /* ignore */
  }
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(Math.max(v, lo), hi);

interface Zone {
  key: keyof LayoutState;
  handle: HTMLElement;
  axis: 'x' | 'y';
  /** +1 si agrandir la zone = déplacer le pointeur vers la droite/le bas, -1 sinon. */
  dir: 1 | -1;
  ratio: boolean;
  /** Dimension totale de référence pour les ratios (px). */
  total: () => number;
  /** Bornes en px. */
  bounds: () => [number, number];
  /** Applique la valeur (px) au DOM. */
  apply: (px: number) => void;
}

const zones: Zone[] = [];

function pxOf(z: Zone): number {
  return z.ratio ? layout[z.key] * z.total() : layout[z.key];
}

function bound(z: Zone, px: number): number {
  const [lo, hi] = z.bounds();
  return clamp(px, lo, Math.max(lo, hi));
}

function toStored(z: Zone, px: number): number {
  return z.ratio ? px / Math.max(1, z.total()) : px;
}

function render(z: Zone): void {
  const [lo, hi] = z.bounds();
  const hiB = Math.max(lo, hi);
  const px = bound(z, pxOf(z));
  z.apply(px);
  const f = z.ratio ? 100 / Math.max(1, z.total()) : 1;
  z.handle.setAttribute('aria-valuenow', String(Math.round(px * f)));
  z.handle.setAttribute('aria-valuemin', String(Math.round(lo * f)));
  z.handle.setAttribute('aria-valuemax', String(Math.round(hiB * f)));
}

function renderAll(): void {
  // Les bornes de la barre latérale et du panneau droit dépendent l'une de l'autre :
  // on re-borne dans l'ordre et on mémorise la valeur effective.
  for (const z of zones) {
    layout[z.key] = toStored(z, bound(z, pxOf(z)));
    render(z);
  }
}

function setPx(z: Zone, px: number, persist: boolean): void {
  layout[z.key] = toStored(z, bound(z, px));
  render(z);
  if (persist) save();
}

function attach(z: Zone): void {
  const h = z.handle;
  let pending = 0;
  let raf = 0;
  let startPos = 0;
  let startPx = 0;
  let dragging = false;

  const flush = (): void => {
    raf = 0;
    setPx(z, pending, false);
  };

  h.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    startPos = z.axis === 'x' ? e.clientX : e.clientY;
    startPx = bound(z, pxOf(z));
    pending = startPx;
    h.setPointerCapture(e.pointerId);
    h.dataset.dragging = 'true';
    document.body.style.userSelect = 'none';
    document.body.style.cursor = z.axis === 'x' ? 'col-resize' : 'row-resize';
  });

  h.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const pos = z.axis === 'x' ? e.clientX : e.clientY;
    pending = startPx + (pos - startPos) * z.dir;
    if (!raf) raf = requestAnimationFrame(flush);
  });

  const end = (e: PointerEvent): void => {
    if (!dragging) return;
    dragging = false;
    if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
    setPx(z, pending, true);
    if (h.hasPointerCapture(e.pointerId)) h.releasePointerCapture(e.pointerId);
    delete h.dataset.dragging;
    document.body.style.userSelect = '';
    document.body.style.cursor = '';
  };
  h.addEventListener('pointerup', end);
  h.addEventListener('pointercancel', end);

  h.addEventListener('dblclick', () => {
    layout[z.key] = LAYOUT_DEFAULTS[z.key];
    setPx(z, pxOf(z), true);
  });

  h.addEventListener('keydown', (e) => {
    const grow = z.axis === 'x' ? 'ArrowRight' : 'ArrowDown';
    const shrink = z.axis === 'x' ? 'ArrowLeft' : 'ArrowUp';
    if (e.key !== grow && e.key !== shrink) return;
    e.preventDefault();
    const step = z.ratio ? KEY_RATIO * z.total() : KEY_PX;
    const sign = (e.key === grow ? 1 : -1) * z.dir;
    setPx(z, bound(z, pxOf(z)) + sign * step, true);
  });
}

function qs<T extends HTMLElement>(sel: string): T | null {
  return document.querySelector<T>(sel);
}

export function initResize(): void {
  layout = load();

  const sidebar = qs('#sidebar');
  const queue = qs('#queue-panel');
  const main = qs('main');
  const gallery = qs('#gallery');
  const top = qs('#queue-top');
  const bottom = qs('#queue-bottom');
  const hSidebar = qs('[data-resize="sidebar"]');
  const hQueue = qs('[data-resize="queue"]');
  const hSplit = qs('[data-resize="queueSplit"]');
  const hGallery = qs('[data-resize="gallery"]');

  const vw = (): number => window.innerWidth;

  if (sidebar && hSidebar) {
    zones.push({
      key: 'sidebar',
      handle: hSidebar,
      axis: 'x',
      dir: 1,
      ratio: false,
      total: () => vw(),
      bounds: () => [SIDEBAR_MIN, Math.min(SIDEBAR_MAX, vw() - layout.queue - MAIN_MIN_W)],
      apply: (px) => {
        sidebar.style.width = `${Math.round(px)}px`;
      },
    });
  }
  if (queue && hQueue) {
    zones.push({
      key: 'queue',
      handle: hQueue,
      axis: 'x',
      dir: -1,
      ratio: false,
      total: () => vw(),
      bounds: () => [QUEUE_MIN, Math.min(QUEUE_MAX, vw() - layout.sidebar - MAIN_MIN_W)],
      apply: (px) => {
        queue.style.width = `${Math.round(px)}px`;
      },
    });
  }
  if (queue && top && bottom && hSplit) {
    zones.push({
      key: 'queueSplit',
      handle: hSplit,
      axis: 'y',
      dir: 1,
      ratio: true,
      total: () => queue.clientHeight,
      bounds: () => [SPLIT_MIN, queue.clientHeight - SPLIT_MIN],
      apply: (px) => {
        const r = px / Math.max(1, queue.clientHeight);
        top.style.flex = `${r} 1 0%`;
        bottom.style.flex = `${1 - r} 1 0%`;
      },
    });
  }
  if (main && gallery && hGallery) {
    zones.push({
      key: 'gallery',
      handle: hGallery,
      axis: 'y',
      dir: -1,
      ratio: true,
      total: () => main.clientHeight,
      bounds: () => [GALLERY_MIN, main.clientHeight - SCREEN_MIN_H],
      apply: (px) => {
        gallery.style.height = `${Math.round(px)}px`;
        gallery.style.flex = '0 0 auto';
      },
    });
  }

  zones.forEach(attach);
  renderAll();

  let raf = 0;
  window.addEventListener('resize', () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      // Au re-bornage on ne réécrit pas les préférences : seules les valeurs
      // effectives affichées changent, les ratios restent ceux de l'utilisateur.
      const saved = { ...layout };
      for (const z of zones) render(z);
      layout = saved;
    });
  });
}
