import { accountLogin, accountLogout, accountStatus, onAccountChanged, errMsg, type AccountStatus, type Provider } from './api';
import { toast } from './toast';
import { state } from './state';
import { renderSourceAccounts } from './sources';

const NAMES: Record<Provider, string> = { x: 'X', reddit: 'Reddit' };
const pending: Record<Provider, boolean> = { x: false, reddit: false };

function render(): void {
  for (const p of ['x', 'reddit'] as const) {
    const info = state.accounts?.[p];
    const status = document.getElementById(`acc-${p}-status`)!;
    const login = document.getElementById(`acc-${p}-login`) as HTMLButtonElement;
    const logout = document.getElementById(`acc-${p}-logout`) as HTMLButtonElement;
    const dot = document.getElementById(`acc-${p}-dot`)!;

    if (pending[p]) {
      status.textContent = 'Fenêtre de connexion ouverte…';
      status.className = 'text-sm text-warning';
      dot.className = 'size-2.5 rounded-full bg-warning animate-pulse';
      login.disabled = true;
      login.textContent = 'En attente…';
      login.hidden = false;
      logout.hidden = true;
    } else if (info?.connected) {
      status.textContent = `Connecté${info.username ? ' · @' + info.username : ''}`;
      status.className = 'text-sm text-success';
      dot.className = 'size-2.5 rounded-full bg-success';
      login.hidden = true;
      logout.hidden = false;
    } else {
      status.textContent = 'Non connecté';
      status.className = 'text-sm text-muted';
      dot.className = 'size-2.5 rounded-full bg-border';
      login.disabled = !state.tauri;
      login.textContent = `Connecter ${NAMES[p]}`;
      login.hidden = false;
      logout.hidden = true;
    }
  }
  renderSourceAccounts();
}

function apply(s: AccountStatus): void {
  state.accounts = s;
  render();
}

export async function initAccounts(): Promise<void> {
  for (const p of ['x', 'reddit'] as const) {
    document.getElementById(`acc-${p}-login`)!.addEventListener('click', async () => {
      pending[p] = true;
      render();
      try {
        apply(await accountLogin(p));
        toast(`Compte ${NAMES[p]} connecté`, 'ok');
      } catch (e) {
        const msg = errMsg(e);
        toast(msg, msg.includes('annulée') ? 'info' : 'err');
      } finally {
        pending[p] = false;
        render();
      }
    });
    document.getElementById(`acc-${p}-logout`)!.addEventListener('click', async () => {
      try {
        apply(await accountLogout(p));
        toast(`Compte ${NAMES[p]} déconnecté`, 'info');
      } catch (e) {
        toast(errMsg(e), 'err');
      }
    });
  }
  render();
  if (!state.tauri) return;
  await onAccountChanged(apply);
  try {
    apply(await accountStatus());
  } catch (e) {
    toast(errMsg(e), 'err');
  }
}
