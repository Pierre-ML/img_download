async function addToQueue() {
  const url   = document.getElementById('url').value.trim();
  const count = parseInt(document.getElementById('count').value, 10);
  const dest  = document.getElementById('dest').value.trim();

  if (!url)              return toast('Entrez une URL Reddit.', 'error');
  if (!url.includes('reddit.com')) return toast('URL Reddit invalide.', 'error');
  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  const res  = await fetch('/api/queue/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'reddit', url, count, dest }),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  document.getElementById('url').value = '';
  toast('Ajouté à la file.', 'success');
}

async function addSavedToQueue() {
  const count = parseInt(document.getElementById('saved-count').value, 10);
  const dest  = document.getElementById('dest').value.trim();

  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  const res  = await fetch('/api/queue/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'reddit_saved', count, dest }),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  toast('Ajouté à la file.', 'success');
}

async function refreshSavedHint() {
  const hint = document.getElementById('saved-account-hint');
  try {
    const res = await fetch('/api/accounts');
    const acc = await res.json();
    if (acc.reddit && acc.reddit.username && acc.reddit.client_id
        && acc.reddit.client_secret_set && acc.reddit.password_set) {
      hint.textContent = `Compte : u/${acc.reddit.username}`;
    }
  } catch { /* on garde le hint par défaut */ }
}

refreshSavedHint();

document.getElementById('url').addEventListener('keydown', e => {
  if (e.key === 'Enter') addToQueue();
});
