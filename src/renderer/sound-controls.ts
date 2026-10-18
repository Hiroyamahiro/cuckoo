import type {ClockStatus,SoundAction,SoundSource} from '../shared/api';
import {soundError} from '../sound-client';
import {previewBlocked,previewLabel,previewMessage} from '../preview-client';
import {soundRecovery} from '../ui-feedback';
import {toast} from './toast';
import {SoundSelect} from './sound-select';
const el=<T extends HTMLElement>(id:string)=>document.getElementById(id) as T;
const pendingNotice='音源操作の処理中、または保存結果が未確定です。「保存結果を確認」で確認できます。';
export class SoundControls {
 private status:ClockStatus|undefined;
 private sources:SoundSource[]=[];
 private key='';private fetchVersion=0;private busy=false;private listError='';private loaded=false;private previewHead='';private listReference='';
 private hour=el<HTMLSelectElement>('hour-sound');private half=el<HTMLSelectElement>('half-sound');
 private hourSelect=new SoundSelect(this.hour);private halfSelect=new SoundSelect(this.half);
 private error=el('sound-error');private notice=el('sound-notice');
 constructor(private run:<T>(task:()=>Promise<T>)=>Promise<T>){
  window.addEventListener('preview-cooldown',()=>this.paint());
  for(const[select,target]of [[this.hour,'hour'],[this.half,'halfHour']] as const)select.addEventListener('change',()=>{const soundId=select.value;this.paint();void this.perform({type:'select',target,soundId});});
  el('sound-register').addEventListener('click',()=>void this.perform({type:'register'}));
  const dialog=el<HTMLDialogElement>('sound-rename-dialog');
  el('sound-rename-cancel').addEventListener('click',()=>{if(!this.busy)dialog.close();});
  dialog.addEventListener('cancel',event=>{if(this.busy)event.preventDefault();});
  el('sound-rename-form').addEventListener('submit',event=>{event.preventDefault();if(this.busy)return;const name=el<HTMLInputElement>('sound-rename-name').value.trim();if(!name){el('sound-rename-error').textContent='登録名を入力してください。';return;}void this.perform({type:'rename',id:dialog.dataset.soundId!,name}).then(saved=>{if(saved)dialog.close();else el('sound-rename-error').textContent=this.error.textContent;});});
  el('sound-refresh').addEventListener('click',()=>void this.refresh());
  el('sound-confirm').addEventListener('click',()=>void this.perform());
  el('hour-preview').addEventListener('click',()=>void this.preview('hour'));
  el('half-preview').addEventListener('click',()=>void this.preview('halfHour'));
 }
 update(status:ClockStatus):void{
  const p=status.audio?.preview,head=p?`${p.id}:${p.state}`:'';
  if(p&&head!==this.previewHead){this.previewHead=head;this.notice.textContent='';if(p.state==='Stuck')this.error.textContent=status.audio?.message||previewMessage(p.state);if(p.state==='Failed')void this.refresh();}
  if(!status.soundManagement?.busy&&!this.busy&&this.notice.textContent===pendingNotice)this.notice.textContent='';
  this.status=status;this.paint();
  const key=JSON.stringify(status.settingsSync.catalogRef);
  if(status.settingsSync.loaded&&status.settingsSync.mode==='persistent'&&key!==this.key){this.key=key;void this.refresh();}
 }
 async refresh():Promise<void>{
  const version=++this.fetchVersion;
  try{const reply=await window.cuckoo.listSources();if(version!==this.fetchVersion)return;
   if(JSON.stringify(reply.catalogRef)!==JSON.stringify(reply.status.settingsSync.catalogRef)&&!reply.error){this.key='';return;}
   this.status=reply.status;
   if(reply.error){this.listError=soundError(reply.error);this.error.textContent=this.listError;}else{this.sources=reply.sources;if(this.error.textContent===this.listError)this.error.textContent='';this.listError='';this.loaded=true;this.listReference=JSON.stringify(reply.catalogRef);}
   this.paint();
  }catch(e){if(version===this.fetchVersion){this.listError=soundError(e);this.error.textContent=this.listError;this.paint();}}
 }
 private async perform(action?:SoundAction):Promise<boolean>{
  if(this.busy)return false;this.busy=true;this.error.textContent='';this.notice.textContent='';this.paint();
  try{const reply=await this.run(()=>action?window.cuckoo.soundOperation(action):window.cuckoo.confirmSoundOperation());this.status=reply.status;
   if(reply.operation.state==='saved'&&!reply.operation.uncertain){if(action?.type==='register')toast('音源を追加しました。');else if(action?.type==='unregister')toast('音源の登録を解除しました。');else if(action?.type==='rename')toast('登録名を変更しました。');}await this.refresh();return reply.operation.state==='saved'&&!reply.operation.uncertain;
  }catch(e){this.notice.textContent='';this.error.textContent=soundError(e);return false;}
  finally{this.busy=false;this.paint();}
 }
 private async preview(target:'hour'|'halfHour'|'one',id?:string):Promise<void>{
  this.error.textContent='';
  try{await window.cuckoo.previewSource(target,id);this.update(await window.cuckoo.getStatus());}
  catch(e){toast(soundError(e),'error');}
  finally{this.paint();}
 }
 private paint():void{
  const s=this.status;if(!s)return;
  const routing=s.settings.audioRouting;
  const listCurrent=this.loaded&&!this.listError&&this.listReference===JSON.stringify(s.settingsSync.catalogRef);
  const writable=this.loaded&&s.settingsSync.canEdit&&!s.soundManagement?.busy&&!this.busy&&!this.listError;
  for(const [select,id,warning,availability]of [[this.hour,routing?.onHour.soundId,'hour-sound-warning',s.audioRoutingAvailability?.onHour],[this.half,routing?.onHalfHour.soundId,'half-sound-warning',s.audioRoutingAvailability?.onHalfHour]] as const){
   const entries=this.sources.slice();if(id&&!entries.some(x=>x.id===id))entries.push({id,name:this.loaded?`利用不能な音源（${id}）`:'保存済みの音源（確認中）',kind:'custom',available:false,pinned:false});
   const signature=JSON.stringify(entries.map(x=>[x.id,x.name,x.available]));
   if(select.dataset.options!==signature){select.replaceChildren(...entries.map(source=>{const option=document.createElement('option');option.value=source.id;option.textContent=source.name+(source.available?'':'（利用不能）');option.disabled=!source.available;return option}));select.dataset.options=signature;}
   if(id)select.value=id;select.disabled=!writable;
   (select===this.hour?this.hourSelect:this.halfSelect).sync();
   const source=entries.find(x=>x.id===id);el(warning).textContent=id&&((this.loaded&&source&&!source.available)||(!listCurrent&&availability?.state==='Unavailable'))?'この音源は利用できません。保存された選択を維持しています。正常な音源へ変更するか、登録状態を確認してください。':'';
  }
  el<HTMLButtonElement>('sound-register').disabled=!writable;
  for(const id of ['sound-rename-name','sound-rename-save'])el<HTMLInputElement>(id).disabled=!writable;
  el<HTMLButtonElement>('sound-rename-cancel').disabled=this.busy;
  for(const [button,id,label]of [['hour-preview',routing?.onHour.soundId,'試聴'],['half-preview',routing?.onHalfHour.soundId,'試聴']]as const){const b=el<HTMLButtonElement>(button);b.disabled=this.busy||previewBlocked(s);b.textContent=previewLabel(s,id,label);}
  const list=el('custom-sounds');const rows=this.sources.filter(x=>x.kind==='custom');el('sound-empty').hidden=rows.length>0;
  // Replace rows only when their meaningful state changes; retain focused buttons.
  const signature=JSON.stringify(rows.map(x=>[x.id,x.name,x.available,x.pinned]))+JSON.stringify(routing)+JSON.stringify(s.settings.customChimes)+String(writable)+String(this.busy)+String(previewBlocked(s))+JSON.stringify(s.audio?.preview)+JSON.stringify(s.audio?.active);
  if(list.dataset.rows!==signature){list.dataset.rows=signature;list.replaceChildren(...rows.map(source=>{
   const li=document.createElement('li');li.dataset.soundId=source.id;
   const name=document.createElement('strong');name.textContent=source.name;
   const detail=document.createElement('span');const selected=[routing?.onHour.soundId===source.id?'00分':'',routing?.onHalfHour.soundId===source.id?'30分':''].filter(Boolean).join('・');
   const playing=s.audio?s.audio.active?.soundId===source.id&&!s.audio.active.outcome:source.pinned;
   detail.textContent=source.available?'':'この音源を利用できません。ファイルや登録状態を確認してください。';
   detail.className='sound-warning';
   detail.hidden=source.available;
   if(!source.available)detail.setAttribute('role','alert');
   const preview=document.createElement('button');preview.type='button';preview.textContent=previewLabel(s,source.id,'試聴');preview.dataset.action='preview';preview.disabled=!source.available||this.busy||previewBlocked(s);preview.addEventListener('click',()=>void this.preview('one',source.id));
   const remove=document.createElement('button');remove.type='button';remove.className='sound-remove';remove.title=selected?`${selected}で使用中。先に別の音源を選んでください。`:playing?'再生準備・再生中のため解除できません。':'登録解除';remove.setAttribute('aria-label',source.name+'の登録を解除');
   const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');svg.setAttribute('viewBox','0 0 24 24');svg.setAttribute('aria-hidden','true');const trash=document.createElementNS(svg.namespaceURI,'path');trash.setAttribute('d','M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7');svg.append(trash);remove.append(svg);remove.dataset.action='unregister';remove.disabled=!writable||!!selected||!!playing;remove.addEventListener('click',()=>{if(window.confirm(`「${source.name}」の登録を解除しますか？ 音源ファイルは復旧用に保持されます。`))void this.perform({type:'unregister',id:source.id});});
   const rename=document.createElement('button');rename.type='button';rename.textContent='登録名を変更';rename.dataset.action='rename';rename.disabled=!writable;rename.addEventListener('click',()=>{const dialog=el<HTMLDialogElement>('sound-rename-dialog');dialog.dataset.soundId=source.id;el<HTMLInputElement>('sound-rename-name').value=source.name;el('sound-rename-error').textContent='';dialog.showModal();el<HTMLInputElement>('sound-rename-name').select();});
   li.append(name,detail,preview,rename,remove);return li;
  }));}
  const unavailable=this.sources.some(x=>!x.available)||(!listCurrent&&(s.audioRoutingAvailability?.onHour.state==='Unavailable'||s.audioRoutingAvailability?.onHalfHour.state==='Unavailable'));
  const recovery=soundRecovery(s,!!this.listError||unavailable,window.cuckoo.soundConfirmationPending(),this.busy);
  el('sound-recovery').hidden=!recovery.visible;el('sound-refresh').hidden=!recovery.refresh;el('sound-confirm').hidden=!recovery.confirm;
  el('sound-recovery-help').textContent=recovery.confirm?'保存の結果がまだ確認できていません。再操作せず、結果を確認してください。':'利用できない音源があります。ファイルや登録状態を確認した後、再確認してください。';
  if(s.soundManagement?.busy&&!this.busy&&!this.notice.textContent)this.notice.textContent=pendingNotice;
  if(s.settingsSync.mode==='blocked')this.error.textContent='復旧が必要なため変更を停止しています。'+s.settingsNotice;
 }
}
