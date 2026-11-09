import type { AccountStatus } from './api';

export const state: {
  tauri: boolean;
  accounts: AccountStatus | null;
} = { tauri: false, accounts: null };
