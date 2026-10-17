import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {ClockStatus} from './shared/api';
import {SettingsSync} from './settings-sync';
import {settingsFeedback} from './ui-feedback';
const sync = new SettingsSync();
const button = document.querySelector<HTMLButtonElement>('#enabled')!;
const label = document.querySelector<HTMLElement>('#power-state')!;
const notice = document.querySelector<HTMLElement>('#notice')!;
let pending = false;
let actionError = '';
const media = matchMedia('(prefers-color-scheme: dark)');
function render(status: ClockStatus): void {
  const s = sync.accept(status);
  const enabled = s.settings.enabled;
  document.documentElement.dataset.theme = s.settings.theme === 'system' ? (media.matches ? 'dark' : 'light') : s.settings.theme;
  button.setAttribute('aria-pressed', String(enabled));
  button.setAttribute('aria-label', enabled ? '時報をOFFにする（現在ON）' : '時報をONにする（現在OFF）');
  button.disabled = pending || !s.settingsSync.canEdit;
  label.textContent = enabled ? '時報ON' : '時報OFF';
  notice.textContent = actionError || settingsFeedback(s).text;
  notice.title = notice.textContent;
}
async function refresh(): Promise<void> { render(await invoke<ClockStatus>('get_status')); }
await listen<ClockStatus>('status', e => render(e.payload));
await refresh();
media.addEventListener('change', () => {if (sync.current) render(sync.current);});
button.addEventListener('click', async () => {
  if (pending) return;
  pending = true; actionError = ''; button.disabled = true;
  try { await invoke('tray_panel_action', {action: 'toggle'}); }
  catch (e) { actionError = String(e); }
  finally { pending = false; await refresh(); }
});
for (const action of ['open', 'quit']) {
  document.querySelector<HTMLButtonElement>('#'+action)!.addEventListener('click', () => {
    void invoke('tray_panel_action', {action}).catch(e => {notice.textContent = String(e);});
  });
}
document.addEventListener('keydown', e => {
  if (e.key === 'Escape') {e.preventDefault(); void invoke('tray_panel_action', {action: 'close'});}
});


