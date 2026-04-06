/* ─────────────────────────────────────────────────────────────────────────
   common.js — partagé entre toutes les pages dashboard
───────────────────────────────────────────────────────────────────────── */

let thumbCount  = 0;
let _toastTimer = null;

/* ── Virtual scroll (décharge les images hors viewport) ──────────────────
   rootMargin:'800px' = zone tampon de 800px au-delà du viewport.
   - isIntersecting → dans la zone : charger l'image
   - !isIntersecting → hors de la zone : vider src pour libérer la mémoire
   Les images restent dans le DOM (la grille ne bouge pas).
──────────────────────────────────────────────────────────────────────── */
const _thumbObserver = new IntersectionObserver((entries) => {
  entries.forEach(e => {
    const img = e.target.querySelector('img');
    if (!img) return;
    if (e.isIntersecting) {
      if (img.dataset.src) {
        img.src = img.dataset.src;
        img.removeAttribute('data-src');
      }
    } else {
      if (img.src && img.src !== window.location.href) {
        img.dataset.src = img.src;
        img.src = '';
      }
    }
  });
}, { rootMargin: '800px' });

/* ── SSE ─────────────────────────────────────────────────────────────── */
(function initStream() {
  const es = new EventSource('/api/stream');

  es.onmessage = ({ data }) => {
    const msg = JSON.parse(data);
    switch (msg.type) {
      case 'queue_update': renderQueue(msg.current, msg.pending); break;
      case 'log':          onLog(msg);      break;
      case 'progress':     onProgress(msg); break;
      case 'thumb':        onThumb(msg);    break;
      case 'done':         onDone(msg);     break;
    }
  };
})();

/* ── Queue ───────────────────────────────────────────────────────────── */
function renderQueue(current, pending) {
  const list  = document.getElementById('queue-list');
  const badge = document.getElementById('queue-badge');
  if (!list) return;

  list.innerHTML = '';
  const total = (current ? 1 : 0) + pending.length;

  if (total === 0) {
    list.innerHTML = '<p class="queue__empty">Aucun job en attente.</p>';
    badge.style.display = 'none';
    _setBtn('btn-stop',   true);
    _setBtn('btn-launch', true);
    setStatus('', 'Prêt');
    return;
  }

  badge.style.display = '';
  badge.textContent   = `${total} job${total > 1 ? 's' : ''}`;

  const running = current !== null;
  _setBtn('btn-stop',   !running);
  _setBtn('btn-launch', running || pending.length === 0);

  if (running) setStatus('running', 'En cours…');

  if (current) {
    const el = document.createElement('div');
    el.className = 'queue__item queue__item--active';
    el.innerHTML = `
      <span class="queue__dot"></span>
      <span class="queue__label">${_esc(current.label)}</span>
      <span class="queue__meta">${current.count}</span>`;
    list.appendChild(el);
  }

  pending.forEach((job, idx) => {
    const el = document.createElement('div');
    el.className = 'queue__item';
    el.innerHTML = `
      <span class="queue__pos">${idx + 1}</span>
      <span class="queue__label">${_esc(job.label)}</span>
      <span class="queue__meta">${job.count}</span>
      <button class="queue__remove" onclick="removeFromQueue(${idx})">✕</button>`;
    list.appendChild(el);
  });
}

async function removeFromQueue(idx) {
  await fetch(`/api/queue/${idx}`, { method: 'DELETE' });
}

async function launchQueue() {
  const res  = await fetch('/api/queue/start', { method: 'POST' });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur', 'error');
  _setBtn('btn-launch', true);
}

async function stopCurrent() {
  await fetch('/api/stop', { method: 'POST' });
  _setBtn('btn-stop', true);
}

/* ── SSE handlers ────────────────────────────────────────────────────── */
function onLog(msg) {
  const box = document.getElementById('log');
  if (!box) return;
  const t    = msg.text || '';
  const line = document.createElement('div');
  line.className = 'log__line' + (
    t.includes('✗') || t.includes('ERREUR') ? ' log__line--error' :
    t.includes('✓')                          ? ' log__line--ok'    :
    t.startsWith('  …')                      ? ' log__line--faint' : '');
  line.textContent = t;
  box.appendChild(line);
  box.scrollTop = box.scrollHeight;
}

function onProgress(msg) {
  const pct  = msg.total ? Math.round(msg.current / msg.total * 100) : 0;
  const fill = document.getElementById('progress-fill');
  const text = document.getElementById('prog-text');
  if (fill) fill.style.width = `${pct}%`;
  if (text) text.textContent = `${msg.current} / ${msg.total}`;
}

function onThumb(msg) {
  addThumb(msg.jobId, msg.filename);
}

function onDone(msg) {
  setStatus(msg.success ? 'done' : 'error', msg.success ? 'Terminé' : 'Erreur');
  toast(msg.message, msg.success ? 'success' : 'error');
}

/* ── Thumbnails ──────────────────────────────────────────────────────── */
function addThumb(jobId, filename) {
  document.getElementById('empty-state')?.remove();

  const url   = `/api/media/${jobId}/${encodeURIComponent(filename)}`;
  const isVid = /\.(mp4|webm|mov|mkv|avi)$/i.test(filename);

  const card = document.createElement('div');
  card.className = 'thumb';

  if (isVid) {
    const icon = document.createElement('div');
    icon.className = 'thumb__overlay';
    icon.style.cssText = 'opacity:1;font-size:28px;background:rgba(0,0,0,.4)';
    icon.textContent = '▷';
    card.appendChild(icon);
    card.style.background = '#111';
  } else {
    const img = document.createElement('img');
    img.dataset.src = url;   // src chargé par _thumbObserver quand en vue
    img.alt         = filename;
    img.decoding    = 'async';
    card.appendChild(img);

    const overlay = document.createElement('div');
    overlay.className   = 'thumb__overlay';
    overlay.textContent = '↗';
    card.appendChild(overlay);
  }

  const name = document.createElement('div');
  name.className   = 'thumb__name';
  name.textContent = filename;

  const del = document.createElement('button');
  del.className   = 'thumb__delete';
  del.textContent = '✕';
  del.title       = 'Supprimer';
  del.addEventListener('click', async (e) => {
    e.stopPropagation();
    await fetch(url, { method: 'DELETE' });
    _thumbObserver.unobserve(card);
    card.remove();
    thumbCount--;
    _updateCount();
  });

  card.append(name, del);
  card.addEventListener('click', () => window.open(url, '_blank'));

  document.getElementById('grid').appendChild(card);
  _thumbObserver.observe(card);   // commence à surveiller la position
  thumbCount++;
  _updateCount();
}

function _updateCount() {
  const el = document.getElementById('count-label');
  if (el) el.textContent = `${thumbCount} fichier${thumbCount > 1 ? 's' : ''}`;
}

/* ── UI helpers ──────────────────────────────────────────────────────── */
function setStatus(variant, label) {
  const badge = document.getElementById('status-badge');
  if (!badge) return;
  badge.className   = `badge${variant ? ' badge--' + variant : ''}`;
  badge.textContent = label;
}

function _setBtn(id, disabled) {
  const el = document.getElementById(id);
  if (el) el.disabled = disabled;
}

function _esc(s) {
  return String(s)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/* ── Reset ───────────────────────────────────────────────────────────── */
async function resetAll() {
  await fetch('/api/reset', { method: 'POST' });

  const log = document.getElementById('log');
  if (log) log.innerHTML = '';

  const fill = document.getElementById('progress-fill');
  if (fill) fill.style.width = '0%';
  const progText = document.getElementById('prog-text');
  if (progText) progText.textContent = '0 / 0';

  const grid = document.getElementById('grid');
  if (grid) {
    grid.querySelectorAll('.thumb').forEach(c => _thumbObserver.unobserve(c));
    grid.innerHTML = '';
  }

  thumbCount = 0;
  _updateCount();
  setStatus('', 'Prêt');
}

/* ── Log toggle ──────────────────────────────────────────────────────── */
function toggleLog() {
  const wrap = document.getElementById('log-wrap');
  const btn  = document.getElementById('btn-log-toggle');
  if (!wrap) return;
  const collapsed = wrap.classList.toggle('log--collapsed');
  if (btn) btn.textContent = collapsed ? 'Afficher' : 'Masquer';
}

/* ── Toast ───────────────────────────────────────────────────────────── */
function toast(msg, type = '') {
  const el = document.getElementById('toast');
  if (!el) return;
  el.textContent = msg;
  el.className   = `toast toast--visible${type ? ' toast--' + type : ''}`;
  clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => { el.className = 'toast'; }, 4500);
}
