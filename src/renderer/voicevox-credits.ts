import type {Settings} from '../shared/settings';
import {legacyVoicevoxCharacters,speechCredit,type Speech} from '../shared/voicevox';
let previous='';
export function renderVoicevoxCredits(settings:Settings):void{
  const voices=[settings.audioRouting?.onHour.speech?.settings,settings.audioRouting?.onHalfHour.speech?.settings,...settings.customChimes.map(c=>c.speech)].filter((s):s is Speech=>!!s);
  const signature=JSON.stringify(voices);if(signature===previous)return;previous=signature;
  const host=document.getElementById('voicevox-used-credits');if(!host)return;
  // Legacy aliases and UUID settings may coexist. Share the credit heading but
  // retain every distinct policy instead of hiding older library conditions.
  const unique=new Map<string,Set<string>>();
  for(const s of voices){const credit=speechCredit(s),policies=unique.get(credit)??new Set<string>();policies.add(s.policy?.trim()||legacyVoicevoxCharacters.find(c=>c.id===s.character)?.terms||'利用規約は時報の編集画面で一覧を更新して確認してください。');unique.set(credit,policies);}
  host.replaceChildren(...[...unique].map(([credit,policies])=>{
    const details=document.createElement('details'),title=document.createElement('summary');title.textContent=credit;
    details.append(title,...[...policies].map(text=>{const policy=document.createElement('p');policy.className='voicevox-credit-policy';policy.textContent=text;return policy;}));return details;
  }));
  if(!unique.size){const p=document.createElement('p');p.textContent='使用中の音声ライブラリはありません。';host.append(p);}
}
