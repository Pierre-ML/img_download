const _SAVED_PLACEHOLDER = '••••••••  (enregistré)';

async function loadAccounts() {
  const res = await fetch('/api/accounts');
  if (!res.ok) return toast('Impossible de charger les comptes.', 'error');
  const acc = await res.json();

  document.getElementById('reddit-client-id').value = acc.reddit.client_id || '';
  document.getElementById('reddit-username').value  = acc.reddit.username  || '';
  document.getElementById('reddit-client-secret').placeholder =
    acc.reddit.client_secret_set ? _SAVED_PLACEHOLDER : '';
  document.getElementById('reddit-password').placeholder =
    acc.reddit.password_set ? _SAVED_PLACEHOLDER : '';

  document.getElementById('x-username').value     = acc.x.username     || '';
  document.getElementById('x-browser').value      = acc.x.browser      || 'firefox';
  document.getElementById('x-cookies-file').value = acc.x.cookies_file || '';
}

async function _save(payload, okMsg) {
  const res  = await fetch('/api/accounts', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  const data = await res.json();
  if (!res.ok) return toast(data.error || 'Erreur serveur', 'error');
  toast(okMsg, 'success');
  loadAccounts();
}

function saveReddit() {
  const payload = { reddit: {
    client_id:     document.getElementById('reddit-client-id').value.trim(),
    client_secret: document.getElementById('reddit-client-secret').value,
    username:      document.getElementById('reddit-username').value.trim().replace(/^u\//, ''),
    password:      document.getElementById('reddit-password').value,
  }};
  if (!payload.reddit.client_id || !payload.reddit.username)
    return toast('ID client et nom d\'utilisateur requis.', 'error');
  _save(payload, 'Compte Reddit enregistré.');
  document.getElementById('reddit-client-secret').value = '';
  document.getElementById('reddit-password').value = '';
}

function saveX() {
  const payload = { x: {
    username:     document.getElementById('x-username').value.trim().replace(/^@/, ''),
    browser:      document.getElementById('x-browser').value,
    cookies_file: document.getElementById('x-cookies-file').value.trim(),
  }};
  if (!payload.x.username)
    return toast('Le pseudo X est requis.', 'error');
  _save(payload, 'Compte X enregistré.');
}

loadAccounts();
