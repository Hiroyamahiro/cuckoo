import type { ClockStatus, SoundSource } from '../shared/api';
import type { CustomChime, Settings } from '../shared/settings';
import { chimeTime, parseChimeTime, putChime, setCustomMinutes } from '../custom-chimes';
import { RouteVolume } from './route-volume';
import { TimeInput } from './time-input';
import { SoundSelect } from './sound-select';
import { previewBlocked, previewLabel } from '../preview-client';
import { soundError } from '../sound-client';
const el = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
export class CustomChimes {
  private status?: ClockStatus;
  private sources: SoundSource[] = [];
  private dialog = el<HTMLDialogElement>('custom-chime-dialog');
  private time = new TimeInput(el('custom-chime-time'));
  private sound = el<HTMLSelectElement>('custom-chime-sound');
  private soundSelect = new SoundSelect(this.sound);
  private error = el('custom-chime-error');
  private editing?: string;
  private busy = false;
  private signature = '';
  private sourceKey = '';
  private dialEdits: Promise<void> = Promise.resolve();
  private volume = new RouteVolume(el('custom-chime-volume'), () => {}, {heading:true});
  constructor(private save: (edit: (settings: Settings) => Partial<Settings>) => Promise<void>) {
    el('custom-chime-preview').addEventListener('click', () => void this.preview());
    this.sound.addEventListener('change', () => { this.soundSelect.sync(); this.paintPreview(); });
    window.addEventListener('preview-cooldown', () => this.paintPreview());
    el('custom-chime-add').addEventListener('click', () => void this.open());
    el('custom-chime-cancel').addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('cancel', e => { if (this.busy) e.preventDefault(); });
    el('custom-chime-form').addEventListener('submit', e => { e.preventDefault(); void this.submit(); });
  }
  update(status: ClockStatus): void {
    this.status = status;
    this.paintPreview();
    const key = JSON.stringify(status.settingsSync.catalogRef);
    if (status.settingsSync.loaded && key !== this.sourceKey) {
      this.sourceKey = key;
      void window.cuckoo.listSources().then(reply => {
        if (!reply.error && key === this.sourceKey) {
          this.sources = reply.sources; this.signature = ''; if (this.status) this.update(this.status);
        }
      }).catch(() => { this.sourceKey = ''; });
    }
    const writable = status.settingsSync.canEdit && !status.soundManagement?.busy && !this.busy;
    el<HTMLButtonElement>('custom-chime-add').disabled = !writable;
    const rows = status.settings.customChimes ?? [];
    const signature = JSON.stringify([rows, writable, status.settingsSync.catalogRef,status.audio?.preview,status.audio?.active,previewBlocked(status)]);
    if (signature === this.signature) return;
    this.signature = signature;
    const list = el('custom-chime-list');
    list.replaceChildren(...[...rows].sort((a,b) => a.minute-b.minute).map(chime => {
      const row = document.createElement('li');
      const toggle = document.createElement('input'); toggle.type = 'checkbox'; toggle.checked = chime.enabled;
      toggle.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を有効にする`); toggle.disabled = !writable;
      toggle.addEventListener('change', () => void this.act(s => ({customChimes: s.customChimes.map(c => c.id === chime.id ? {...c, enabled: toggle.checked} : c)})));
      const edit = document.createElement('button'); edit.type = 'button'; edit.className = 'custom-chime-edit';
      edit.textContent = chimeTime(chime.minute); edit.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を編集`); edit.disabled = !writable;
      edit.addEventListener('click', () => void this.open(chime.id));
      const details = document.createElement('span'); details.className = 'custom-chime-detail';
      const source = this.sources.find(s => s.id === chime.soundId);
      details.textContent = `${source?.name ?? '音色を確認中'} · ${chime.repeat}回 · ${chime.volume}%`;
      if (source && !source.available) details.textContent += '（利用不能）';
      const remove = document.createElement('button'); remove.type = 'button'; remove.textContent = '削除'; remove.disabled = !writable;
      remove.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を削除`);
      remove.addEventListener('click', () => void this.act(s => ({customChimes: s.customChimes.filter(c => c.id !== chime.id)})));
      const preview = document.createElement('button'); preview.type='button'; preview.className='sound-preview';
      preview.textContent=previewLabel(status,chime.soundId,'試聴'); preview.disabled=this.busy || previewBlocked(status) || source?.available === false;
      preview.setAttribute('aria-label',`${chimeTime(chime.minute)}の時報を試聴`);
      preview.addEventListener('click', () => void this.preview(chime));
      row.append(toggle, edit, details, preview, remove); return row;
    }));
  }
  private async open(id?: string): Promise<void> {
    if (this.busy) return;
    this.busy = true; this.error.textContent = '';
    try {
      const reply = await window.cuckoo.listSources();
      if (reply.error) throw new Error('音源一覧を取得できません。音色の追加から登録状態を確認してください。');
      this.sources = reply.sources;
      const chime = id ? reply.status.settings.customChimes.find(c => c.id === id) : undefined;
      if (id && !chime) return;
      this.editing = id;
      this.time.value = chimeTime(chime?.minute ?? 9 * 60);
      this.sound.replaceChildren(...this.sources.map(source => {
        const option = document.createElement('option'); option.value = source.id;
        option.textContent = source.name + (source.available ? '' : '（利用不能）'); option.disabled = !source.available; return option;
      }));
      if (chime && !this.sources.some(s => s.id === chime.soundId)) {
        const missing = new Option('利用不能な音源', chime.soundId); missing.disabled = true; this.sound.add(missing);
      }
      this.sound.value = chime?.soundId ?? reply.status.settings.audioRouting?.onHour.soundId ?? this.sources[0]?.id ?? '';
      this.soundSelect.sync();
      this.volume.sync(chime?.volume ?? 100, true);
      for (const radio of document.querySelectorAll<HTMLInputElement>('[name="custom-repeat"]')) radio.checked = Number(radio.value) === (chime?.repeat ?? 1);
      el('custom-chime-title').textContent = id ? '時報の編集' : '時報の追加';
      this.dialog.showModal(); this.time.focus();
    } catch (e) { el('input-error').textContent = String(e); }
    finally { this.busy = false; this.paintPreview(); }
  }
  async setMinutes(minutes: readonly number[], enabled: boolean): Promise<void> {
    const pending = this.dialEdits.then(() => this.act(s => ({customChimes:setCustomMinutes(s.customChimes,minutes,enabled)})));
    this.dialEdits = pending.catch(() => {});
    await pending;
  }
  private paintPreview(): void {
    const button=el<HTMLButtonElement>('custom-chime-preview');
    button.disabled=this.busy || !this.status || previewBlocked(this.status) || !this.sound.value || this.sound.selectedOptions[0]?.disabled;
    button.textContent=this.status ? previewLabel(this.status,this.sound.value,'試聴') : '試聴';
  }
  private async preview(chime?: CustomChime): Promise<void> {
    const id=chime?.soundId ?? this.sound.value;
    const repeat=chime?.repeat ?? Number(document.querySelector<HTMLInputElement>('[name="custom-repeat"]:checked')!.value) as 1|2;
    const volume=chime?.volume ?? Number(this.volume.input.value);
    try { await window.cuckoo.previewSource('custom',id,repeat,volume); this.update(await window.cuckoo.getStatus()); }
    catch (e) { if(this.dialog.open) this.error.textContent=soundError(e); else el('input-error').textContent=soundError(e); }
    finally { this.paintPreview(); }
  }
  private async act(edit: (settings: Settings) => Partial<Settings>): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try { await this.save(edit); }
    catch (e) { el('input-error').textContent = String(e); }
    finally { this.busy = false; if (this.status) this.update(this.status); }
  }
  private async submit(): Promise<void> {
    if (this.busy) return;
    this.error.textContent = '';
    let chime: CustomChime;
    try {
      chime = {id: this.editing ?? crypto.randomUUID(), minute: parseChimeTime(this.time.value), enabled: this.status?.settings.customChimes.find(c => c.id === this.editing)?.enabled ?? true,
        soundId: this.sound.value, repeat: Number(document.querySelector<HTMLInputElement>('[name="custom-repeat"]:checked')!.value) as 1|2, volume: Number(this.volume.input.value)};
      if (!chime.soundId) throw new Error('音色を選択してください。');
      putChime(this.status?.settings.customChimes ?? [], chime);
    } catch (e) { this.error.textContent = String(e).replace(/^Error: /, ''); return; }
    this.busy = true;
    this.setBusy(true);
    try {
      await this.save(s => {
        if (this.editing && !s.customChimes.some(c => c.id === this.editing)) throw new Error('この時報は削除されています。');
        return {customChimes: putChime(s.customChimes, chime)};
      });
      this.dialog.close();
    } catch (e) { this.error.textContent = String(e).replace(/^Error: /, ''); }
    finally { this.busy = false; this.setBusy(false); if (this.status) this.update(this.status); }
  }
  private setBusy(value: boolean): void {
    for (const control of this.dialog.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement>('input,button,select')) control.disabled = value;
    this.soundSelect.sync();
    this.paintPreview();
  }
}
