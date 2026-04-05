async function addToQueue() {
  const raw  = document.getElementById('urls').value;
  const dest = document.getElementById('dest').value.trim();
  const urls = raw.split('\n').map(u => u.trim()).filter(Boolean);

  if (!urls.length) return toast('Entrez au moins une URL.', 'error');

  const res  = await fetch('/api/queue/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'video', urls, dest }),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  document.getElementById('urls').value = '';
  toast(`${urls.length} lien(s) ajouté(s).`, 'success');
}
