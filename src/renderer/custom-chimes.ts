import type { ClockStatus, SoundSource } from '../shared/api';
import type { CustomChime, Settings } from '../shared/settings';
import { chimeTime, parseChimeTime, putChime, setCustomMinutes } from '../custom-chimes';
import { RouteVolume } from './route-volume';
import { TimeInput } from './time-input';
import { SoundSelect } from './sound-select';
import { VoicevoxControls } from './voicevox-controls';
import {speechName} from '../shared/voicevox';
import {renderVoicevoxCredits} from './voicevox-credits';
import {trashIcon} from './trash-icon';
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
  private route?: 'hour'|'halfHour';
  private busy = false;
  private signature = '';
  private sourceKey = '';
  private dialEdits: Promise<void> = Promise.resolve();
  private volume = new RouteVolume(el('custom-chime-volume'), () => {}, {heading:true});
  private speech = new VoicevoxControls(() => parseChimeTime(this.time.value), () => this.paintPreview(),()=>this.route?Array.from({length:24},(_,hour)=>hour*60+(this.route==='hour'?0:30)):[parseChimeTime(this.time.value)],()=>{
    this.sound.value=this.previousSound||this.sources.find(s=>s.available)?.id||'';this.soundSelect.sync();this.speech.show(false);
  });
  private previousSound='';
  constructor(private save: (edit: (settings: Settings) => Partial<Settings>) => Promise<void>) {
    el('custom-chime-preview').addEventListener('click', () => void this.preview());
    this.sound.addEventListener('change', () => { if(this.sound.value!=='voicevox')this.previousSound=this.sound.value;this.soundSelect.sync(); this.speech.show(this.sound.value === 'voicevox'); });
    el('custom-chime-time').addEventListener('change', () => this.speech.sync());
    window.addEventListener('preview-cooldown', () => this.paintPreview());
    el('custom-chime-add').addEventListener('click', () => void this.open());
    for(const [id,target] of [['hour','hour'],['half','halfHour']] as const){
      el(id+'-edit').addEventListener('click',()=>void this.open(undefined,target));
      el(id+'-preview').addEventListener('click',()=>void window.cuckoo.previewSource(target).catch(e=>{el('input-error').textContent=soundError(e);}));
    }
    el('custom-chime-cancel').addEventListener('click', () => this.dialog.close());
    this.dialog.addEventListener('cancel', e => { if (this.busy || this.speech.busy) e.preventDefault(); });
    el('custom-chime-form').addEventListener('submit', e => { e.preventDefault(); void this.submit(); });
  }
  update(status: ClockStatus): void {
    this.status = status;
    renderVoicevoxCredits(status.settings);
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
    for(const [id,key,repeat] of [['hour','onHour',status.settings.hourlyRepeat],['half','onHalfHour',status.settings.halfHourlyRepeat]] as const){
      const route=status.settings.audioRouting?.[key],speech=route?.speech?.settings;
      const name=speech?speechName(speech):this.sources.find(s=>s.id===route?.soundId)?.name??'音色を確認中';
      el(id+'-detail').textContent=`${name} · ${repeat}回 · ${route?.volume??100}%`;
      el<HTMLButtonElement>(id+'-edit').disabled=!writable;
      const preview=el<HTMLButtonElement>(id+'-preview');preview.disabled=previewBlocked(status)||this.busy;
      const minute=new Date().getHours()*60+(id==='hour'?0:30);
      preview.textContent=previewLabel(status,route?.speech?.clips[minute]??route?.soundId,'試聴');
    }
    el<HTMLButtonElement>('custom-chime-add').disabled = !writable;
    const rows = status.settings.customChimes ?? [];
    const signature = JSON.stringify([rows, writable, status.settingsSync.catalogRef,status.audio?.preview,status.audio?.active,previewBlocked(status)]);
    if (signature === this.signature) return;
    this.signature = signature;
    const list = el('custom-chime-list');
    list.replaceChildren(...[...rows].sort((a,b) => a.minute-b.minute).map(chime => {
      const row = document.createElement('li');
      row.className='chime-row';
      const toggle = document.createElement('input'); toggle.type = 'checkbox'; toggle.checked = chime.enabled;
      toggle.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を有効にする`); toggle.disabled = !writable;
      toggle.addEventListener('change', () => void this.act(s => ({customChimes: s.customChimes.map(c => c.id === chime.id ? {...c, enabled: toggle.checked} : c)})));
      const edit = document.createElement('button'); edit.type = 'button'; edit.className = 'custom-chime-edit chime-edit';
      edit.textContent = chimeTime(chime.minute); edit.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を編集`); edit.disabled = !writable;
      edit.addEventListener('click', () => void this.open(chime.id));
      const details = document.createElement('span'); details.className = 'custom-chime-detail chime-detail';
      const source = this.sources.find(s => s.id === chime.soundId);
      const name=chime.speech ? speechName(chime.speech) : source?.name ?? '音色を確認中';
      details.textContent = `${name} · ${chime.repeat}回 · ${chime.volume}%`;
      if (source && !source.available) details.textContent += '（利用不能）';
      const remove = document.createElement('button'); remove.type = 'button'; remove.className = 'sound-remove'; remove.append(trashIcon()); remove.title = '削除'; remove.disabled = !writable;
      remove.setAttribute('aria-label', `${chimeTime(chime.minute)}の時報を削除`);
      remove.addEventListener('click', () => void this.act(s => ({customChimes: s.customChimes.filter(c => c.id !== chime.id)})));
      const preview = document.createElement('button'); preview.type='button'; preview.className='sound-preview';
      preview.textContent=previewLabel(status,chime.soundId,'試聴'); preview.disabled=this.busy || previewBlocked(status) || source?.available === false;
      preview.setAttribute('aria-label',`${chimeTime(chime.minute)}の時報を試聴`);
      preview.addEventListener('click', () => void this.preview(chime));
      row.append(toggle, edit, details, preview, remove); return row;
    }));
  }
  private async open(id?: string,route?:'hour'|'halfHour'): Promise<void> {
    if (this.busy) return;
    this.busy = true; this.error.textContent = '';
    try {
      const reply = await window.cuckoo.listSources();
      if (reply.error) throw new Error('音源一覧を取得できません。音色の追加から登録状態を確認してください。');
      this.sources = reply.sources;
      const chime = id ? reply.status.settings.customChimes.find(c => c.id === id) : undefined;
      if (id && !chime) return;
      this.editing = id;
      this.route=route;
      const routing=route?reply.status.settings.audioRouting?.[route==='hour'?'onHour':'onHalfHour']:undefined;
      const speech=route?routing?.speech?.settings:chime?.speech;
      this.time.value = chimeTime(chime?.minute ?? (route?new Date().getHours():9)*60+(route==='halfHour'?30:0));
      el('chime-time-field').hidden=!!route;
      const groups=Array.from({length:3},()=>document.createElement('optgroup'));
      groups[0].label='通常のチャイム';groups[1].label='登録した音色';groups[2].label='VOICEVOX';
      for(const source of this.sources){
        const option = document.createElement('option'); option.value = source.id;
        option.textContent = source.name + (source.available ? '' : '（利用不能）'); option.disabled = !source.available;
        groups[source.id.startsWith('builtin:')?0:1].append(option);
      }
      groups[2].append(new Option('音声読み上げ（VOICEVOX）','voicevox'));
      this.sound.replaceChildren(...groups.filter(g=>g.children.length));
      if (chime && !chime.speech && !this.sources.some(s => s.id === chime.soundId)) {
        const missing = new Option('利用不能な音源', chime.soundId); missing.disabled = true; this.sound.add(missing);
      }
      this.sound.value = speech ? 'voicevox' : chime?.soundId ?? routing?.soundId ?? reply.status.settings.audioRouting?.onHour.soundId ?? this.sources[0]?.id ?? '';
      this.previousSound=this.sound.value==='voicevox'?this.sources.find(s=>s.available)?.id??'':this.sound.value;
      this.speech.configure(speech,chime?.soundId,routing?.speech?.clips);
      this.speech.show(this.sound.value === 'voicevox');
      this.soundSelect.sync();
      this.volume.sync(chime?.volume ?? routing?.volume ?? 100, true);
      const repeat=route==='hour'?reply.status.settings.hourlyRepeat:route==='halfHour'?reply.status.settings.halfHourlyRepeat:chime?.repeat??1;
      for (const radio of document.querySelectorAll<HTMLInputElement>('[name="custom-repeat"]')) radio.checked = Number(radio.value) === repeat;
      el('custom-chime-title').textContent = route?`${route==='hour'?'00分':'30分'}の時報`:id ? '時報の編集' : '時報の追加';
      this.dialog.showModal(); if(route)this.soundSelect.focus();else this.time.focus();
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
    const busy=this.busy || this.speech.busy;
    const voice=this.sound.value==='voicevox';
    const repeats=document.querySelectorAll<HTMLInputElement>('[name="custom-repeat"]');
    el('custom-repeat-title').parentElement!.hidden=voice;
    for(const radio of repeats){if(voice)radio.checked=radio.value==='1';radio.disabled=busy||voice;}
    for(const id of ['custom-chime-cancel','custom-chime-save','custom-chime-time-hour','custom-chime-time-minute'])el<HTMLButtonElement>(id).disabled=busy;
    this.sound.disabled=busy;this.soundSelect.sync();
    button.disabled=busy || !this.status || previewBlocked(this.status) || !this.sound.value || this.sound.selectedOptions[0]?.disabled;
    button.textContent=this.speech.busy?'準備中…':this.status ? previewLabel(this.status,voice?this.speech.soundId:this.sound.value,'試聴') : '試聴';
  }
  private async preview(chime?: CustomChime): Promise<void> {
    if(this.dialog.open)this.error.textContent='';
    let id=chime?.soundId ?? this.sound.value;
    const repeat=chime?.repeat ?? Number(document.querySelector<HTMLInputElement>('[name="custom-repeat"]:checked')!.value) as 1|2;
    const volume=chime?.volume ?? Number(this.volume.input.value);
    try { if(!chime && id==='voicevox')id=await this.speech.ensure(); await window.cuckoo.previewSource('custom',id,repeat,volume); this.update(await window.cuckoo.getStatus()); }
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
    if (this.busy || this.speech.busy) return;
    this.error.textContent = '';
    let chime: CustomChime;
    try {
      if(this.route){
        const hour=this.route==='hour',repeat=Number(document.querySelector<HTMLInputElement>('[name="custom-repeat"]:checked')!.value) as 1|2,volume=Number(this.volume.input.value);
        const patch:Partial<Settings>=hour?{hourlyRepeat:repeat,hourVolume:volume}:{halfHourlyRepeat:repeat,halfHourVolume:volume};
        if(this.sound.value==='voicevox'){
          await this.speech.ensure();const settings=this.speech.value;
          if(hour)patch.hourVoice={settings,clips:this.speech.clips};else patch.halfHourVoice={settings,clips:this.speech.clips};
        }else if(hour)patch.hourSoundId=this.sound.value;else patch.halfHourSoundId=this.sound.value;
        await this.persist(()=>patch);return;
      }
      chime = {id: this.editing ?? crypto.randomUUID(), minute: parseChimeTime(this.time.value), enabled: this.status?.settings.customChimes.find(c => c.id === this.editing)?.enabled ?? true,
        soundId: this.sound.value, repeat: Number(document.querySelector<HTMLInputElement>('[name="custom-repeat"]:checked')!.value) as 1|2, volume: Number(this.volume.input.value)};
      putChime(this.status?.settings.customChimes ?? [], chime);
      if(this.sound.value==='voicevox'){
        chime.soundId=await this.speech.ensure();
        chime.speech=this.speech.value;
      }
      if (!chime.soundId) throw new Error('音色を選択してください。');
    } catch (e) { this.error.textContent = String(e).replace(/^Error: /, ''); return; }
    await this.persist(s => {
        if (this.editing && !s.customChimes.some(c => c.id === this.editing)) throw new Error('この時報は削除されています。');
        return {customChimes: putChime(s.customChimes, chime)};
    });
  }
  private async persist(edit:(settings:Settings)=>Partial<Settings>):Promise<void>{
    this.busy=true;this.setBusy(true);
    try {
      await this.save(edit);
      this.dialog.close();
    } catch (e) { this.error.textContent = String(e).replace(/^Error: /, ''); }
    finally { this.busy = false; this.setBusy(false); this.speech.sync(); if (this.status) this.update(this.status); }
  }
  private setBusy(value: boolean): void {
    for (const control of this.dialog.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement>('input,button,select,textarea')) control.disabled = value;
    this.soundSelect.sync();
    this.paintPreview();
  }
}
