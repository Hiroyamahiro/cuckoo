import {legacyVoicevoxCharacters,type Speech} from '../shared/voicevox';

export function voicevoxPolicyDisplay(voice?:Pick<Speech,'character'|'policy'>):{credit:string;url:string|null}{
  const legacy=legacyVoicevoxCharacters.find(c=>c.id===voice?.character);
  const policy=voice?.policy??'';
  const credit=policy.match(/[「｢“"]\s*(VOICEVOX[:：][^\r\n」｣”"]{1,200})[」｣”"]/)?.[1]?.trim()??legacy?.credit??'';
  const urls=[...new Set((policy.match(/https?:\/\/[^\s<>"「」]+/g)??[]).map(value=>value.replace(/[。）、),.;]+$/u,'')))].filter(value=>{
    try{const url=new URL(value);return !!url.hostname&&!url.username&&!url.password;}catch{return false;}
  });
  const terms=urls.filter(value=>/terms|kiyaku|guideline|利用規約/i.test(value));
  return {credit,url:terms.length===1?terms[0]:urls.length===1?urls[0]:legacy?.terms??null};
}

export function renderVoicevoxPolicy(host:HTMLElement,voice?:Pick<Speech,'character'|'policy'>):void{
  const {credit,url}=voicevoxPolicyDisplay(voice);
  host.replaceChildren();
  if(credit){const text=document.createElement('p');text.textContent=credit;host.append(text);}
  if(url){
    const line=document.createElement('p'),link=document.createElement('a');
    link.textContent='利用規約';link.href=url;
    link.addEventListener('click',event=>{
      event.preventDefault();
      void window.cuckoo.openCredit(`voicevox-policy:${url}`).catch(()=>{});
    });
    line.append(link);host.append(line);
  }
}
