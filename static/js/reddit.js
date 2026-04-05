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

document.getElementById('url').addEventListener('keydown', e => {
  if (e.key === 'Enter') addToQueue();
});
