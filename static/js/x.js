async function refreshAccountHint() {
  const hint = document.getElementById('x-account-hint');
  try {
    const res = await fetch('/api/accounts');
    const acc = await res.json();
    if (acc.x && acc.x.username) {
      hint.textContent = `@${acc.x.username} — cookies ${
        acc.x.cookies_file ? 'fichier' : acc.x.browser}`;
    } else {
      hint.innerHTML =
        'Aucun compte configuré — <a href="/comptes" style="color:var(--clr-accent)">page Comptes</a>';
    }
  } catch {
    hint.textContent = 'Compte : erreur de chargement';
  }
}

async function addToQueue() {
  const count = parseInt(document.getElementById('count').value, 10);
  const dest  = document.getElementById('dest').value.trim();

  if (!count || count < 1) return toast('Nombre invalide.', 'error');

  const res  = await fetch('/api/queue/add', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ type: 'x_likes', count, dest }),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');

  toast('Ajouté à la file.', 'success');
}

refreshAccountHint();
