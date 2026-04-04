/* ─────────────────────────────────────────────────────────────────────────
   State
───────────────────────────────────────────────────────────────────────── */
let currentJobId = null;
let thumbCount   = 0;

/* ─────────────────────────────────────────────────────────────────────────
   SSE — connexion globale unique
───────────────────────────────────────────────────────────────────────── */
(function initStream() {
  const es = new EventSource('/api/stream');

  es.onmessage = ({ data }) => {
    const msg = JSON.parse(data);

    switch (msg.type) {

      case 'queue_update':
        renderQueue(msg.current, msg.pending);
        break;

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
        addThumb(msg.jobId, msg.filename);
        break;

      case 'done':
        setStatus(msg.success ? 'done' : 'error', msg.success ? 'Terminé' : 'Erreur');
        toast(msg.message, msg.success ? 'success' : 'error');
        break;
    }
  };

  es.onerror = () => {
    // Reconnexion automatique gérée par le navigateur (EventSource)
  };
})();

/* ─────────────────────────────────────────────────────────────────────────
   Ajouter à la file
───────────────────────────────────────────────────────────────────────── */
async function addToQueue() {
  const keyword = document.getElementById('keyword').value.trim();
  const count   = parseInt(document.getElementById('count').value, 10);
  const dest    = document.getElementById('dest').value.trim();

  if (!keyword)            return toast('Entrez un mot-clé.', 'error');
  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  let res;
  try {
    res = await fetch('/api/queue/add', {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body:    JSON.stringify({ keyword, count, dest }),
    });
  } catch {
    return toast('Impossible de contacter le serveur.', 'error');
  }

  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  // Vider le champ mot-clé pour faciliter l'ajout du suivant
  document.getElementById('keyword').value = '';
  document.getElementById('keyword').focus();
  toast(`"${keyword}" ajouté à la file.`, 'success');
}

/* ─────────────────────────────────────────────────────────────────────────
   Supprimer un job en attente
───────────────────────────────────────────────────────────────────────── */
async function removeFromQueue(idx) {
  await fetch(`/api/queue/${idx}`, { method: 'DELETE' });
}

/* ─────────────────────────────────────────────────────────────────────────
   Lancer la file
───────────────────────────────────────────────────────────────────────── */
async function launchQueue() {
  const res  = await fetch('/api/queue/start', { method: 'POST' });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur', 'error');
  document.getElementById('btn-launch').disabled = true;
}

/* ─────────────────────────────────────────────────────────────────────────
   Stop job courant
───────────────────────────────────────────────────────────────────────── */
async function stopCurrent() {
  await fetch('/api/stop', { method: 'POST' });
  document.getElementById('btn-stop').disabled = true;
}

/* ─────────────────────────────────────────────────────────────────────────
   Rendu de la file d'attente
───────────────────────────────────────────────────────────────────────── */
function renderQueue(current, pending) {
  const list  = document.getElementById('queue-list');
  const badge = document.getElementById('queue-badge');
  list.innerHTML = '';

  const total = (current ? 1 : 0) + pending.length;

  if (total === 0) {
    list.innerHTML = '<p class="queue-empty">Aucun téléchargement en cours.</p>';
    badge.style.display = 'none';
    document.getElementById('btn-stop').disabled   = true;
    document.getElementById('btn-launch').disabled = true;
    setStatus('', 'Prêt');
    return;
  }

  // Bouton Lancer : actif seulement si file non vide et rien en cours
  const isRunning = current !== null;
  document.getElementById('btn-launch').disabled = isRunning || pending.length === 0;

  // Badge total
  badge.style.display = '';
  badge.textContent   = `${total} job${total > 1 ? 's' : ''}`;

  // Job courant
  if (current) {
    currentJobId = current.jobId;
    document.getElementById('btn-stop').disabled = false;
    setStatus('running', 'En cours…');

    const el = document.createElement('div');
    el.className = 'queue-item queue-item--active';
    el.innerHTML = `
      <span class="queue-item__dot"></span>
      <span class="queue-item__kw">${escHtml(current.keyword)}</span>
      <span class="queue-item__count">${current.count} img</span>
    `;
    list.appendChild(el);
  }

  // Jobs en attente
  pending.forEach((job, idx) => {
    const el = document.createElement('div');
    el.className = 'queue-item';
    el.innerHTML = `
      <span class="queue-item__pos">${idx + 1}</span>
      <span class="queue-item__kw">${escHtml(job.keyword)}</span>
      <span class="queue-item__count">${job.count} img</span>
      <button class="queue-item__remove" onclick="removeFromQueue(${idx})" title="Retirer">✕</button>
    `;
    list.appendChild(el);
  });
}

/* ─────────────────────────────────────────────────────────────────────────
   Thumbnail
───────────────────────────────────────────────────────────────────────── */
function addThumb(jobId, filename) {
  document.getElementById('empty-state')?.remove();

  const url  = `/api/thumb/${jobId}/${encodeURIComponent(filename)}`;
  const card = document.createElement('div');
  card.className = 'thumb-card';

  const img = document.createElement('img');
  img.src      = url;
  img.alt      = filename;
  img.loading  = 'lazy';
  img.decoding = 'async';

  const overlay = document.createElement('div');
  overlay.className   = 'thumb-overlay';
  overlay.textContent = '↗';

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
    document.getElementById('count-label').textContent =
      `${thumbCount} image${thumbCount > 1 ? 's' : ''}`;
  });

  card.append(img, overlay, name, del);
  card.addEventListener('click', () => window.open(url, '_blank'));

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
function setStatus(variant, label) {
  const badge = document.getElementById('status-badge');
  badge.className   = `badge${variant ? ` ${variant}` : ''}`;
  badge.textContent = label;
}

function escHtml(str) {
  return str.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
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
  if (e.key === 'Enter') addToQueue();
});
