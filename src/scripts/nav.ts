export const SCREENS = ['google', 'x', 'reddit', 'accounts', 'settings'] as const;
export type Screen = (typeof SCREENS)[number];

export function showScreen(name: Screen): void {
  document.querySelectorAll<HTMLElement>('[data-screen]').forEach((el) => {
    el.hidden = el.dataset.screen !== name;
  });
  document.querySelectorAll<HTMLElement>('[data-nav]').forEach((el) => {
    el.setAttribute('aria-current', String(el.dataset.nav === name));
  });
}

export function initNav(): void {
  document.querySelectorAll<HTMLElement>('[data-nav]').forEach((el) => {
    el.addEventListener('click', () => showScreen(el.dataset.nav as Screen));
  });
  showScreen('google');
}
