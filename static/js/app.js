/* ─────────────────────────────────────────────────────────────────────────
   State
───────────────────────────────────────────────────────────────────────── */
let currentJobId = null;
let currentES    = null;
let thumbCount   = 0;
let targetCount  = 0;

/* ─────────────────────────────────────────────────────────────────────────
   Start
───────────────────────────────────────────────────────────────────────── */
async function startDownload() {
  const keyword = document.getElementById('keyword').value.trim();
  const count   = parseInt(document.getElementById('count').value, 10);
  const dest    = document.getElementById('dest').value.trim();

  if (!keyword)            return toast('Entrez un mot-clé.', 'error');
  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  targetCount = count;
  resetUI(count);
  setStatus('running', 'En cours…');

  let res;
  try {
    res = await fetch('/api/start', {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body:    JSON.stringify({ keyword, count, dest }),
    });
  } catch {
    toast('Impossible de contacter le serveur.', 'error');
    setStatus('error', 'Erreur');
    setRunning(false);
    return;
  }

  const data = await res.json();
  if (!res.ok) {
    toast(data.error || 'Erreur serveur', 'error');
    setStatus('error', 'Erreur');
    setRunning(false);
    return;
  }

  currentJobId = data.jobId;
  openStream(currentJobId);
}

/* ─────────────────────────────────────────────────────────────────────────
   Stop
───────────────────────────────────────────────────────────────────────── */
async function stopDownload() {
  if (!currentJobId) return;
  if (currentES) { currentES.close(); currentES = null; }
  await fetch(`/api/stop/${currentJobId}`, { method: 'POST' });
  setRunning(false);
  setStatus('', 'Arrêté');
  log('Arrêté par l\'utilisateur.', 'faint');
}

/* ─────────────────────────────────────────────────────────────────────────
   SSE
───────────────────────────────────────────────────────────────────────── */
function openStream(jobId) {
  if (currentES) currentES.close();

  const es = new EventSource(`/api/stream/${jobId}`);
  currentES = es;

  es.onmessage = ({ data }) => {
    const msg = JSON.parse(data);

    switch (msg.type) {
      case 'log':
        log(msg.text,
          msg.text.includes('✗') || msg.text.includes('ERREUR') ? 'error'
          : msg.text.includes('✓') ? 'ok'
          : msg.text.startsWith('  …') ? 'faint'
          : ''
        );
        break;

      case 'progress':
        setProgress(msg.current, msg.total);
        break;

      case 'thumb':
        addThumb(jobId, msg.filename);
        break;

      case 'done':
        es.close();
        currentES = null;
        setRunning(false);
        setStatus(msg.success ? 'done' : 'error', msg.success ? 'Terminé' : 'Erreur');
        toast(msg.message, msg.success ? 'success' : 'error');
        break;
    }
  };
}

/* ─────────────────────────────────────────────────────────────────────────
   Thumbnail
───────────────────────────────────────────────────────────────────────── */
function addThumb(jobId, filename) {
  document.getElementById('empty-state')?.remove();

  const card = document.createElement('div');
  card.className = 'thumb-card';

  const img = document.createElement('img');
  img.src     = `/api/thumb/${jobId}/${encodeURIComponent(filename)}`;
  img.alt     = filename;
  img.loading = 'lazy';
  img.decoding = 'async';

  const overlay = document.createElement('div');
  overlay.className   = 'thumb-overlay';
  overlay.textContent = '↗';

  const name = document.createElement('div');
  name.className   = 'thumb-name';
  name.textContent = filename;

  card.append(img, overlay, name);
  card.addEventListener('click', () => {
    window.open(`/api/thumb/${jobId}/${encodeURIComponent(filename)}`, '_blank');
  });

  document.getElementById('grid').appendChild(card);
  thumbCount++;
  document.getElementById('count-label').textContent =
    `${thumbCount} image${thumbCount > 1 ? 's' : ''}`;
}

/* ─────────────────────────────────────────────────────────────────────────
   Log
───────────────────────────────────────────────────────────────────────── */
function log(text, variant = '') {
  const box  = document.getElementById('log');
  const line = document.createElement('div');
  line.className   = `line${variant ? ` line--${variant}` : ''}`;
  line.textContent = text;
  box.appendChild(line);
  box.scrollTop = box.scrollHeight;
}

/* ─────────────────────────────────────────────────────────────────────────
   Progress
───────────────────────────────────────────────────────────────────────── */
function setProgress(current, total) {
  const pct = total ? Math.round(current / total * 100) : 0;
  document.getElementById('progress-fill').style.width = `${pct}%`;
  document.getElementById('progressbar')?.setAttribute('aria-valuenow', pct);
  document.getElementById('prog-text').textContent = `${current} / ${total}`;
}

/* ─────────────────────────────────────────────────────────────────────────
   UI helpers
───────────────────────────────────────────────────────────────────────── */
function resetUI(count) {
  // Vider le log
  document.getElementById('log').innerHTML = '';
  // Vider la grille
  const grid = document.getElementById('grid');
  grid.innerHTML = '';
  thumbCount = 0;

  const empty = document.createElement('div');
  empty.className = 'empty-state';
  empty.id = 'empty-state';
  empty.innerHTML = `
    <div class="empty-state__icon" aria-hidden="true">⏳</div>
    <p>Téléchargement en cours…</p>
  `;
  grid.appendChild(empty);

  // Reset progression
  setProgress(0, count);
  document.getElementById('count-label').textContent = '0 image';
  setRunning(true);
}

function setRunning(running) {
  document.getElementById('btn-start').disabled = running;
  document.getElementById('btn-stop').disabled  = !running;
}

function setStatus(variant, label) {
  const badge = document.getElementById('status-badge');
  badge.className = `badge${variant ? ` ${variant}` : ''}`;
  badge.textContent = label;
}

/* ─────────────────────────────────────────────────────────────────────────
   Toast
───────────────────────────────────────────────────────────────────────── */
let _toastTimer = null;

function toast(msg, type = '') {
  const el = document.getElementById('toast');
  el.textContent = msg;
  el.className   = `toast show${type ? ` toast--${type}` : ''}`;
  clearTimeout(_toastTimer);
  _toastTimer = setTimeout(() => { el.className = 'toast'; }, 4500);
}

/* ─────────────────────────────────────────────────────────────────────────
   Keyboard
───────────────────────────────────────────────────────────────────────── */
document.getElementById('keyword').addEventListener('keydown', (e) => {
  if (e.key === 'Enter') startDownload();
});
