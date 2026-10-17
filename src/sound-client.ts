import type {ClockStatus,SoundAction,SoundReply} from './shared/api';
interface Request {sessionId:string;baseRevision:number;requestId:string;action:SoundAction}
export function soundError(error:unknown):string {
 const code=String(error);
 for(const [key,message] of [
  ['SoundSelected','この音源は00分または30分で使用中です。先に別の音源を選んで保存してください。'],
  ['SoundPinned','再生準備・再生中の音源は解除できません。再生終了後に確認してください。'],
  ['SettingsWriteBlockedOrDirty','設定が未保存、または保存処理中です。変更を破棄せず、保存状態を確認してください。'],
  ['RecoveryRequired','復旧が必要です。保存成功とは確認できないため、変更を停止しています。'],
  ['CommitUncertain','保存結果が未確定です。登録解除や再保存をせず、結果を確認してください。'],
  ['InvalidSoundName','登録名は1〜80文字で入力してください。'],
  ['InjectedUiSaveFailure','保存に失敗しました。音源の変更は反映していません。保存状態を確認してから再操作してください。'],
  ['CountLimit','音源の登録は100件までです。不要な登録を解除してください。'],
  ['CapacityLimit','音源の保存領域が上限に達しています。新しい音源は登録していません。'],
  ['FileSizeLimit','MP3は25MiB以下のファイルを選んでください。'],
  ['Duration','再生時間は60秒以下のMP3を選んでください。'],
  ['Decode','MP3を読み取れません。形式またはファイルの破損を確認してください。'],
  ['BlobHash','音源の内容が登録時と異なります。別の正常な音源を選んでください。'],
  ['UnknownId','未登録または解除済みの音源です。選択を自動変更していません。'],
  ['NotFound','音源ファイルが見つかりません。選択を自動変更していません。'],
  ['AudioManagementBusy','前の音源操作を処理中です。結果を確認してください。'],
  ['RevisionConflict','設定が更新されています。最新状態を確認して操作してください。'],
  ['ChimeImminent','時報直前のため試聴できません。時報後にお試しください。'],
  ['AudioStuck','音声処理の停止を確認できません。新しい再生を開始しません。'],
  ['PreviewCooldown','試聴ボタンは300ms空けて操作してください。'],
  ['AutomaticPriority','時報の再生を優先しています。終了後に試聴してください。'],
 ] as const) if(code.includes(key))return message;
 return `操作を完了できませんでした：${code}`;
}
// A lost response retains the SAME operation ID. Never retry a write with a new ID.
export class SoundClient {
 get needsConfirmation():boolean{return this.pending!==undefined;}
 private pending:Request|undefined;
 constructor(private getStatus:()=>Promise<ClockStatus>,private send:(request:Request)=>Promise<SoundReply>,private read:(sessionId:string,requestId:string)=>Promise<SoundReply>,private accept:(status:ClockStatus)=>ClockStatus,private pause=()=>new Promise<void>(r=>setTimeout(r,150))){}
 async run(action:SoundAction):Promise<SoundReply>{
  if(this.pending){await this.confirm();throw Error('前の操作結果を確認しました。最新状態を確認してから操作してください。')}
  let status=await this.getStatus();
  for(let i=0;i<40 && status.settingsSync.mode==='persistent' && status.settingsSync.savedRevision!==status.settingsSync.revision && status.saveState!=='error';i++){await this.pause();status=await this.getStatus();}
  if(status.settingsSync.mode!=='persistent'||!status.settingsSync.canEdit||status.settingsSync.savedRevision!==status.settingsSync.revision)throw Error('SettingsWriteBlockedOrDirty');
  const {sessionId,revision:baseRevision}=status.settingsSync;
  this.pending={sessionId,baseRevision,requestId:crypto.randomUUID(),action};
  const reply=await this.send(this.pending);this.acceptReply(reply);
  return this.resolve(reply);
 }
 private acceptReply(reply:SoundReply):void{if(this.pending && reply.requestId!==this.pending.requestId)throw Error('音源操作の応答が一致しません。');reply.status=this.accept(reply.status);}
 private async resolve(reply:SoundReply):Promise<SoundReply>{
  for(let i=0;reply.operation.state==='pending'&&i<40;i++){await this.pause();reply=await this.read(this.pending!.sessionId,this.pending!.requestId);this.acceptReply(reply);}
  if(reply.operation.state==='pending')throw Error('処理を継続しています。「結果を確認」で同じ操作の結果を確認してください。');
  if(reply.operation.uncertain)throw Error('CommitUncertain');
  this.pending=undefined;
  if(reply.operation.state==='failed'||reply.operation.state==='rejected')throw Error(reply.operation.code??'操作に失敗しました');
  return reply;
 }
 async confirm():Promise<SoundReply>{
  if(!this.pending){const s=await this.getStatus(),operation=s.soundManagement?.operation;if(!operation)throw Error('確認する音源操作はありません。');this.pending={sessionId:s.settingsSync.sessionId,baseRevision:s.settingsSync.revision,requestId:operation.requestId,action:{type:'register'}};}
  const reply=await this.read(this.pending.sessionId,this.pending.requestId);this.acceptReply(reply);return this.resolve(reply);
 }
}
