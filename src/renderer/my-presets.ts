import {
  presetNameLimit,
  type NamedPreset,
  type PresetAction,
} from '../shared/settings';
import {toast} from './toast';
import {CustomSelect} from './sound-select';
const element = <T extends HTMLElement>(id: string): T =>
  document.getElementById(id) as T;
export class MyPresets {
  private presets: NamedPreset[] = [];
  private readonly select = element<HTMLSelectElement>('preset-select');
  private readonly selector=new CustomSelect(this.select,{icons:false,placeholder:'プリセットを選択'});
  private readonly dialog = element<HTMLDialogElement>('preset-dialog');
  private readonly name = element<HTMLInputElement>('preset-name');
  private readonly menu = element<HTMLElement>('preset-menu');
  private readonly manage = element<HTMLButtonElement>('preset-manage');
  private action: 'create' | 'rename' | 'overwrite' | 'delete' | 'apply' =
    'create';
  private target = '';
  private busy = false;
  private writable = false;
  private slots: readonly boolean[] = [];
  private returnFocus: HTMLElement | null = null;
  constructor(
    private readonly operate: (action: PresetAction) => Promise<string>,
  ) {
    this.name.maxLength = presetNameLimit;
    element('preset-save').addEventListener('click', () => this.open('create'));
    this.select.addEventListener('change', () => {
      if (!this.select.value) {
        this.disable(this.busy);
        return;
      }
      void this.run({ type: 'apply', id: this.select.value });
    });
    element('preset-overwrite').addEventListener('click', () =>
      this.open('overwrite'),
    );
    this.manage.addEventListener('click', () => {
      const open = this.menu.hidden;
      this.menu.hidden = !open;
      this.manage.setAttribute('aria-expanded', String(open));
      if (open) this.items()[0]?.focus();
    });
    this.manage.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        if (this.menu.hidden) this.manage.click();
        else this.items()[0]?.focus();
      }
    });
    this.menu.addEventListener('keydown', (event) => {
      const items = this.items();
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        items[
          (index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) %
            items.length
        ]?.focus();
      }
    });
    for (const button of this.menu.querySelectorAll<HTMLButtonElement>(
      'button',
    ))
      button.addEventListener('click', () => {
        this.closeMenu();
        this.open(button.dataset.presetAction as typeof this.action);
      });
    document.addEventListener('pointerdown', (event) => {
      if (
        !this.menu.contains(event.target as Node) &&
        !this.manage.contains(event.target as Node)
      )
        this.closeMenu();
    });
    document.addEventListener('focusin', (event) => {
      if (
        !this.menu.contains(event.target as Node) &&
        event.target !== this.manage
      )
        this.closeMenu();
    });
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && !this.menu.hidden) {
        event.preventDefault();
        this.closeMenu();
        this.manage.focus();
      }
      if (event.key === 'Tab') this.closeMenu();
    });
    element('preset-cancel').addEventListener('click', () => {
      if (!this.busy) this.dialog.close();
    });
    this.dialog.addEventListener('cancel', (event) => {
      if (this.busy) event.preventDefault();
    });
    this.dialog.addEventListener('close', () => {
      const origin = this.returnFocus;
      if (
        origin &&
        !origin.matches(':disabled') &&
        origin.getClientRects().length
      )
        origin.focus();
      else if (!element('preset-controls').hidden && !this.select.disabled)
        this.selector.focus();
      else element('preset-save').focus();
    });
    element('preset-form').addEventListener('submit', (event) => {
      event.preventDefault();
      if (this.busy || !this.writable) return;
      const name = this.name.value.trim();
      if (this.action === 'create' || this.action === 'rename') {
        if (!name || name.length > presetNameLimit) {
          this.error(`名前は1〜${presetNameLimit}文字で入力してください。`);
          return;
        }
        // Rust owns uniqueness. In particular, an operation that timed out may
        // already be saved; its receipt must be queried before rejecting a retry.
      }
      void this.run(
        this.action === 'create'
          ? { type: 'create', name }
          : this.action === 'rename'
            ? { type: 'rename', id: this.target, name }
            : { type: this.action, id: this.target },
      );
    });
    this.disable(false);
  }
  private items(): HTMLButtonElement[] {
    return [
      ...this.menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'),
    ];
  }
  private closeMenu(): void {
    this.menu.hidden = true;
    this.manage.setAttribute('aria-expanded', 'false');
  }
  private error(message: string): void {
    element('preset-error').textContent = message;
  }
  private open(action: typeof this.action): void {
    if (this.busy || !this.writable) return;
    const selected = this.presets.find(
      (preset) => preset.id === this.select.value,
    );
    if (action !== 'create' && !selected) return;
    if ((action === 'apply' || action === 'overwrite') && !this.hasDifference())
      return;
    this.action = action;
    this.target = selected?.id ?? '';
    this.returnFocus =
      action === 'create'
        ? element('preset-save')
        : action === 'overwrite'
          ? element('preset-overwrite')
          : this.manage;
    this.error('');
    const naming = action === 'create' || action === 'rename';
    const labels = {
      create: 'プリセットを保存',
      rename: '名前変更',
      overwrite: '上書き',
      delete: '削除',
      apply: '保存内容を再適用',
    };
    element('preset-dialog-title').textContent = labels[action];
    element('preset-dialog-description').textContent = naming
      ? `名前は${presetNameLimit}文字以内で入力してください。`
      : action === 'overwrite'
        ? `「${selected!.name}」を現在の時計盤で上書きしますか？`
        : action === 'apply'
          ? `現在の編集内容を「${selected!.name}」の保存内容で置き換えます。プリセットに保存していない編集内容は失われます。再適用しますか？`
          : `「${selected!.name}」を削除しますか？`;
    element('preset-name-label').hidden = !naming;
    this.name.disabled = !naming;
    this.name.value = action === 'rename' ? selected!.name : '';
    element('preset-confirm').textContent =
      action === 'create' ? '保存' : labels[action];
    this.dialog.showModal();
    if (naming) {
      this.name.focus();
      this.name.select();
    } else element('preset-cancel').focus();
  }
  private async run(action: PresetAction): Promise<void> {
    if (this.busy || !this.writable) return;
    this.busy = true;
    this.disable(true);
    try {
      const id = await this.operate(action);
      if (action.type !== 'delete') this.select.value = id;
      this.dialog.close();
      if(action.type!=='apply')toast(action.type==='delete'?'プリセットを削除しました。':action.type==='rename'?'名前を変更しました。':'プリセットを保存しました。');
    } catch (error) {
      const message = `プリセット操作に失敗しました：${String(error)}`;
      if (this.dialog.open) this.error(message);
      else element('input-error').textContent = message;
    } finally {
      this.busy = false;
      this.disable(false);
    }
  }
  private disable(value: boolean): void {
    const blocked=value || !this.writable;
    for (const id of [
      'preset-save',
      'preset-overwrite',
      'preset-manage',
      'preset-confirm',
    ])
      element<HTMLButtonElement>(id).disabled = blocked;
    element<HTMLButtonElement>('preset-cancel').disabled=value;
    this.select.disabled = blocked;
    this.selector.sync();
    const selected = this.presets.find(
      (preset) => preset.id === this.select.value,
    );
    const different = this.hasDifference();
    element<HTMLButtonElement>('preset-overwrite').disabled =
      blocked || !different;
    element<HTMLButtonElement>('preset-reapply').disabled = blocked || !different;
    element('preset-difference').hidden = !different;
    this.manage.disabled = blocked || !selected;
  }
  private hasDifference(): boolean {
    const selected = this.presets.find(
      (preset) => preset.id === this.select.value,
    );
    return (
      !!selected &&
      !selected.slots.every((slot, index) => slot === this.slots[index])
    );
  }
  setSlots(slots: readonly boolean[]): void {
    this.slots = [...slots];
    this.disable(this.busy);
  }
  setWritable(writable:boolean):void {this.writable=writable;this.disable(this.busy);}
  sync(presets: NamedPreset[], slots?: readonly boolean[]): void {
    if (slots) this.slots = [...slots];
    const selected = this.select.value;
    if (JSON.stringify(presets) !== JSON.stringify(this.presets)) {
      this.presets = presets;
      this.select.replaceChildren(
        new Option('プリセットを選択', ''),
        ...presets.map((preset) => new Option(preset.name, preset.id)),
      );
      if (presets.some((preset) => preset.id === selected))
        this.select.value = selected;
    }
    element('preset-empty').hidden = presets.length > 0;
    element('preset-controls').hidden = presets.length === 0;
    this.disable(this.busy);
  }
}
