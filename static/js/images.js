async function addToQueue() {
  const keyword = document.getElementById('keyword').value.trim();
  const count   = parseInt(document.getElementById('count').value, 10);
  const dest    = document.getElementById('dest').value.trim();

  if (!keyword)            return toast('Entrez un mot-clé.', 'error');
  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  const res  = await fetch('/api/queue/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'images', keyword, count, dest }),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  document.getElementById('keyword').value = '';
  document.getElementById('keyword').focus();
  toast(`"${keyword}" ajouté.`, 'success');
}

document.getElementById('keyword').addEventListener('keydown', e => {
  if (e.key === 'Enter') addToQueue();
});
