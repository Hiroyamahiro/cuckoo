import type { UpdateStatus } from './shared/api';

export function updateMenu(status: UpdateStatus) {
  const busy = status.phase === 'checking' || status.phase === 'downloading' || status.phase === 'launched';
  const percent = status.total > 0 ? Math.min(100, Math.floor(status.received * 100 / status.total)) : 0;
  return {
    dot: !!status.version,
    disabled: busy,
    cancel: status.phase === 'downloading',
    label: status.phase === 'checking' ? '確認中…'
      : status.phase === 'downloading' ? `ダウンロード中 ${percent}%`
      : status.phase === 'launched' ? 'インストーラーで更新を続ける'
      : status.version ? `更新する v${status.version}` : '更新を確認',
    action: status.version ? 'install' as const : 'check' as const,
    message: status.message,
  };
}

export class UpdateMenu {
  private current: UpdateStatus = { seq: -1, phase: 'idle', version: null, received: 0, total: 0, message: '' };
  private readonly button = document.querySelector<HTMLButtonElement>('#update-action')!;
  private readonly cancel = document.querySelector<HTMLButtonElement>('#cancel-update')!;
  constructor() {
    window.cuckoo.onUpdateStatus(s => this.accept(s));
    void window.cuckoo.getUpdateStatus().then(s => this.accept(s)).catch(() => {});
    this.button.addEventListener('click', () => { void window.cuckoo.updateAction(updateMenu(this.current).action).catch(() => this.failure()); });
    this.cancel.addEventListener('click', () => { void window.cuckoo.updateAction('cancel').catch(() => this.failure()); });
  }
  private failure() { document.querySelector('#update-message')!.textContent = '更新処理を開始できませんでした。再試行できます'; }
  private accept(status: UpdateStatus) {
    if (status.seq <= this.current.seq) return;
    this.current = status;
    const view = updateMenu(status);
    document.querySelector('#update-dot')!.toggleAttribute('hidden', !view.dot);
    this.button.querySelector('#update-label')!.textContent = view.label;
    this.button.disabled = view.disabled;
    this.cancel.hidden = !view.cancel;
    document.querySelector('#update-message')!.textContent = view.message;
    document.querySelector('#app-menu-button')!.setAttribute('aria-label', view.dot ? 'Cuckooメニュー（更新あり）' : 'Cuckooメニュー');
  }
}
