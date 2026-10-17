import type {ClockStatus} from './shared/api';
let until=0;
export function previewCooling(now=performance.now()):boolean{return now<until;}
export function claimPreviewClick(now=performance.now()):void{
 if(previewCooling(now))throw Error('PreviewCooldown');
 until=now+300;
 window.dispatchEvent(new Event('preview-cooldown'));
 setTimeout(()=>window.dispatchEvent(new Event('preview-cooldown')),305);
}
export function previewBlocked(s:ClockStatus):boolean{
 return previewCooling()||!!s.soundManagement?.busy||!s.audio||s.audio.canPreview===false||['Unknown','Stopped','Stuck'].includes(s.audio.state)||!!s.audio.active&&s.audio.active.kind!=='Test';
}
export function previewLabel(s:ClockStatus,id:string|undefined,fallback:string):string{
 const p=s.audio?.preview;
 if(id&&s.audio?.active?.kind==='Test'&&s.audio.active.soundId===id&&s.audio.active.phase!=='Stuck')return '停止';
 if(id&&p?.soundId===id&&['Queued','Switching'].includes(p.state))return '切り替え中';
 return fallback;
}
export function previewMessage(state:string):string{
 return ({Queued:'試聴を準備しています。',Switching:'前の試聴の停止を確認してから切り替えます。',Playing:'試聴中です。同じ音源を押すと停止します。',Stopping:'試聴の停止を確認しています。',Stopped:'試聴を停止しました。',Completed:'再生が完了しました。',Interrupted:'試聴を中断しました。時報は常に優先されます。',Skipped:'試聴は再生されませんでした。',Failed:'試聴に失敗しました。',Stuck:'停止を確認できません。新しい音声処理は開始しません。'} as Record<string,string>)[state]??'';
}
