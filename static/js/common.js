/* ─────────────────────────────────────────────────────────────────────────
   common.js — partagé entre toutes les pages dashboard
   Fournit : SSE, queue render, thumb, log, progress, toast, stop, launch
───────────────────────────────────────────────────────────────────────── */

let thumbCount   = 0;
let _toastTimer  = null;

/* ── SSE ─────────────────────────────────────────────────────────────── */
(function initStream() {
  const es = new EventSource('/api/stream');

  es.onmessage = ({ data }) => {
    const msg = JSON.parse(data);
    switch (msg.type) {
      case 'queue_update': renderQueue(msg.current, msg.pending); break;
      case 'log':      onLog(msg);      break;
      case 'progress': onProgress(msg); break;
      case 'thumb':    onThumb(msg);    break;
      case 'done':     onDone(msg);     break;
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
    list.innerHTML = '<p class="queue-empty">Aucun job en attente.</p>';
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
    el.className = 'queue-item queue-item--active';
    el.innerHTML = `
      <span class="queue-item__dot"></span>
      <span class="queue-item__kw">${_esc(current.label)}</span>
      <span class="queue-item__meta">${current.count}</span>`;
    list.appendChild(el);
  }

  pending.forEach((job, idx) => {
    const el = document.createElement('div');
    el.className = 'queue-item';
    el.innerHTML = `
      <span class="queue-item__pos">${idx + 1}</span>
      <span class="queue-item__kw">${_esc(job.label)}</span>
      <span class="queue-item__meta">${job.count}</span>
      <button class="queue-item__remove" onclick="removeFromQueue(${idx})">✕</button>`;
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
  const box  = document.getElementById('log');
  if (!box) return;
  const line = document.createElement('div');
  const t    = msg.text || '';
  line.className = 'line' + (
    t.includes('✗') || t.includes('ERREUR') ? ' line--error' :
    t.includes('✓')                          ? ' line--ok'    :
    t.startsWith('  …')                      ? ' line--faint' : '');
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

  const url  = `/api/media/${jobId}/${encodeURIComponent(filename)}`;
  const isVid = /\.(mp4|webm|mov|mkv|avi)$/i.test(filename);

  const card = document.createElement('div');
  card.className = 'thumb-card';

  if (isVid) {
    const icon = document.createElement('div');
    icon.className = 'thumb-overlay';
    icon.style.cssText = 'opacity:1;font-size:28px;background:rgba(0,0,0,.4)';
    icon.textContent = '▷';
    card.appendChild(icon);
    card.style.background = '#111';
  } else {
    const img = document.createElement('img');
    img.src      = url;
    img.alt      = filename;
    img.loading  = 'lazy';
    img.decoding = 'async';
    card.appendChild(img);

    const overlay = document.createElement('div');
    overlay.className   = 'thumb-overlay';
    overlay.textContent = '↗';
    card.appendChild(overlay);
  }

  const name = document.createElement('div');
  name.className   = 'thumb-name';
  name.textContent = filename;

  const del = document.createElement('button');
  del.className   = 'thumb-delete';
  del.textContent = '✕';
  del.title       = 'Supprimer';
  del.addEventListener('click', async (e) => {
    e.stopPropagation();
    await fetch(url, { method: 'DELETE' });
    card.remove();
    thumbCount--;
    _updateCount();
  });

  card.append(name, del);
  card.addEventListener('click', () => window.open(url, '_blank'));

  document.getElementById('grid').appendChild(card);
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
  badge.className   = `badge${variant ? ' ' + variant : ''}`;
  badge.textContent = label;
}

function _setBtn(id, disabled) {
  const el = document.getElementById(id);
  if (el) el.disabled = disabled;
}

function _esc(s) {
  return String(s)
    .replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
}

/* ── Toast ───────────────────────────────────────────────────────────── */
function toast(msg, type = '') {
  const el = document.getElementById('toast');
  if (!el) return;
  el.textContent = msg;
  el.className   = `toast show${type ? ' toast--' + type : ''}`;
  clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => { el.className = 'toast'; }, 4500);
}
