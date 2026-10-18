import {CustomSelect} from './sound-select';
import {voicevoxThumbnail} from './voicevox-thumbnails';
import {renderVoicevoxPolicy} from './voicevox-policy';
import {legacyVoicevoxCharacters,speechName,speechSignature,type Speech,type VoicevoxCharacter} from '../shared/voicevox';
export class VoicevoxControls {
  private host=document.getElementById('voicevox-controls')!;
  private character=document.getElementById('voicevox-character') as HTMLSelectElement;
  private mode=document.getElementById('voicevox-mode') as HTMLSelectElement;
  private style=document.getElementById('voicevox-style') as HTMLSelectElement;
  private search=document.getElementById('voicevox-search') as HTMLInputElement;
  private listStatus=document.getElementById('voicevox-list-status')!;
  private guide=document.getElementById('voicevox-guide')!;
  private speakers:VoicevoxCharacter[]=[];
  private voice?:Pick<Speech,'character'|'styleId'|'characterName'|'styleName'|'policy'>;
  private loading=false;
  private request=0;
  private policyRequest=0;
  private text=document.getElementById('voicevox-text') as HTMLTextAreaElement;
  private message=document.getElementById('voicevox-status')!;
  private selects:CustomSelect[];
  private generated?:{signature:string;clips:Record<string,string>;verified:boolean};
  private pending?:Promise<string>;
  private lease=crypto.randomUUID();
  private protect(ids:string[]):Promise<void>{return window.cuckoo.protectSpeechEditor(this.lease,[...new Set(ids)]);}
  constructor(private minute:()=>number,private changed:()=>void,private minutes:()=>number[]=()=>[this.minute()],private cancel:()=>void=()=>{}){
    this.selects=[new CustomSelect(this.character,{thumbnail:id=>{
      const legacy=legacyVoicevoxCharacters.find(c=>c.id===id);
      const speaker=this.speakers.find(s=>s.uuid===id||(legacy&&s.name===legacy.name));
      const style=speaker?.styles.find(s=>speaker.uuid===this.voice?.character?s.id===this.voice.styleId:s.name===legacy?.style)??speaker?.styles[0];
      return speaker&&style?voicevoxThumbnail(speaker.uuid,style.id,speaker.version):Promise.resolve(null);
    }}),new CustomSelect(this.style,{icons:false}),new CustomSelect(this.mode,{icons:false})];
    for(const control of [this.mode,this.text])control.addEventListener(control===this.text?'input':'change',()=>{this.message.textContent='';this.sync();});
    this.character.addEventListener('change',()=>{
      const speaker=this.speakers.find(s=>s.uuid===this.character.value);if(!speaker)return;
      this.choose(speaker,speaker.styles[0].id);this.renderChoices();void this.loadPolicy();
    });
    this.style.addEventListener('change',()=>{const speaker=this.speakers.find(s=>s.uuid===this.voice?.character);if(speaker)this.choose(speaker,Number(this.style.value));this.renderChoices();});
    this.search.addEventListener('input',()=>this.renderChoices());
    for(const id of ['voicevox-refresh','voicevox-reconnect'])document.getElementById(id)!.addEventListener('click',()=>void this.refresh());
    document.getElementById('voicevox-download')!.addEventListener('click',()=>void window.cuckoo.openCredit('voicevoxDownload').catch(e=>{this.message.textContent=String(e);}));
    document.getElementById('voicevox-cancel')!.addEventListener('click',()=>this.cancel());
    this.host.closest('dialog')?.addEventListener('close',()=>{this.request++;this.policyRequest++;this.loading=false;void this.protect([]).catch(()=>{});});
  }
  private choose(speaker:VoicevoxCharacter,id:number):void{
    const style=speaker.styles.find(s=>s.id===id);if(!style)return;
    if(this.voice?.character!==speaker.uuid){this.policyRequest++;}
    this.voice={character:speaker.uuid,styleId:id,characterName:speaker.name,styleName:style.name,policy:this.voice?.character===speaker.uuid?this.voice.policy:undefined};
  }
  private renderChoices():void{
    const query=this.search.value.trim().normalize('NFKC').toLocaleLowerCase();
    const matching=this.speakers.filter(s=>s.name.normalize('NFKC').toLocaleLowerCase().includes(query));
    const options=matching.map(s=>{const option=new Option(`${s.name}（${s.styles.map(v=>v.name).join('・')}）`,s.uuid);option.dataset.thumbnailKey=JSON.stringify([s.version,s.uuid===this.voice?.character?this.voice.styleId:s.styles[0].id]);return option;});
    if(this.voice&&!options.some(o=>o.value===this.voice!.character))options.unshift(new Option(`${this.voice.characterName??legacyVoicevoxCharacters.find(c=>c.id===this.voice?.character)?.name??this.voice.character}（現在の設定）`,this.voice.character));
    this.character.replaceChildren(...options);this.character.value=this.voice?.character??'';
    const speaker=this.speakers.find(s=>s.uuid===this.voice?.character);
    const styles=speaker?.styles.map(s=>new Option(s.name,String(s.id)))??[];
    const styleId=this.voice?.styleId===undefined?'saved':String(this.voice.styleId);
    if(this.voice&&!styles.some(s=>s.value===styleId))styles.unshift(new Option(`${this.voice.styleName??'保存済みのスタイル'}（現在の設定）`,styleId));
    this.style.replaceChildren(...styles);this.style.value=styleId;
    if(!this.loading&&this.guide.hidden)this.listStatus.textContent=this.speakers.length?query&&!matching.length?'該当するキャラクターがありません':`${matching.length}キャラクター`: '読み上げ可能なキャラクターがありません';
    this.sync();
  }
  async refresh():Promise<void>{
    if(this.loading||this.busy)return;
    const request=++this.request;this.loading=true;this.listStatus.textContent='読み込み中…';this.sync();
    try{
      const reply=await window.cuckoo.checkVoicevox();if(request!==this.request)return;
      this.speakers=reply.characters;this.guide.hidden=true;
      if(!this.voice&&this.speakers.length)this.choose(this.speakers[0],this.speakers[0].styles[0].id);
      this.loading=false;this.renderChoices();void this.loadPolicy();
    }catch{if(request!==this.request)return;this.loading=false;this.guide.hidden=false;this.listStatus.textContent='未接続';this.sync();}
  }
  private async loadPolicy():Promise<void>{
    const voice=this.voice;if(!voice||voice.styleId===undefined)return;
    const request=++this.policyRequest;
    this.sync();
    try {const policy=await window.cuckoo.voicevoxPolicy(voice.character);if(!policy.trim())throw new Error('EmptyVoicevoxPolicy');if(request===this.policyRequest&&this.voice?.character===voice.character){this.voice.policy=policy;this.sync();}}
    catch{if(request===this.policyRequest){this.sync();}}
  }
  get visible():boolean{return !this.host.hidden;}
  get busy():boolean{return !!this.pending;}
  private signature(speech:Speech):string{return JSON.stringify(this.minutes().map(minute=>[minute,speechSignature(speech,minute)]));}
  get soundId():string|undefined{try{return this.generated?.signature===this.signature(this.value)?this.generated.clips[this.minute()]:undefined;}catch{return undefined;}}
  get clips():Record<string,string>{return {...this.generated?.clips};}
  get value():Speech{return{...this.voice,character:this.voice?.character??'',mode:this.mode.value as Speech['mode'],text:this.mode.value==='time'?'':this.text.value};}
  configure(speech?:Speech,soundId?:string,clips?:Record<string,string>):void {
    this.request++;this.policyRequest++;this.loading=false;this.search.value='';this.guide.hidden=true;
    this.voice=speech?{character:speech.character,styleId:speech.styleId,characterName:speech.characterName,styleName:speech.styleName,policy:speech.policy}:undefined;
    (document.getElementById('voicevox-policy') as HTMLDetailsElement).open=false;
    this.mode.value=speech?.mode??'time';this.text.value=speech?.text??'';
    this.generated=speech&&(soundId||clips)?{signature:this.signature(speech),clips:clips??{[this.minute()]:soundId!},verified:false}:undefined;
    void this.protect(Object.values(this.generated?.clips??{})).catch(()=>{});
    this.message.textContent='';this.renderChoices();
  }
  show(value:boolean):void{this.host.hidden=!value;this.sync();if(value)void this.refresh();}
  sync():void{
    const busy=this.busy;
    this.character.disabled=busy||this.loading;this.style.disabled=busy||this.loading;this.search.disabled=busy||this.loading;this.mode.disabled=busy;this.text.disabled=busy;
    for(const id of ['voicevox-refresh','voicevox-reconnect']) (document.getElementById(id) as HTMLButtonElement).disabled=busy||this.loading;
    this.text.required=this.visible&&this.mode.value!=='time';
    document.getElementById('voicevox-text-field')!.hidden=this.mode.value==='time';
    this.host.setAttribute('aria-busy',String(busy));this.selects.forEach(s=>s.sync());this.changed();
    document.getElementById('voicevox-selection')!.textContent=this.voice?speechName(this.value):'キャラクターを選択してください';
    renderVoicevoxPolicy(document.getElementById('voicevox-policy-content')!,this.voice);
  }
  async ensure():Promise<string>{
    if(this.pending)return this.pending;
    const speech=this.value,minutes=this.minutes(),signature=this.signature(speech);
    if(!this.voice)throw new Error('VOICEVOXに接続してキャラクターを選択してください。');
    // Saved clips can be validated/reused offline. New voices retain their library policy.
    this.message.textContent='音声を準備しています…';
    this.pending=(async()=>{
      if(speech.styleId!==undefined&&!speech.policy&&this.generated?.signature!==signature){
        speech.policy=await window.cuckoo.voicevoxPolicy(speech.character);
        if(!speech.policy.trim())throw new Error('利用規約を取得できません。一覧を更新して再試行してください。');
        if(this.voice?.character===speech.character){this.voice.policy=speech.policy;}
      }
      const clips:Record<string,string>={},unique=new Map<string,string>();
      for(const [index,minute] of minutes.entries()){
        this.message.textContent=minutes.length>1?`音声を準備しています… ${index+1}/${minutes.length}`:'音声を準備しています…';
        const key=speechSignature(speech,minute);let id=unique.get(key);
        if(!id){const reply=await window.cuckoo.generateSpeech(speech,minute,false);id=reply.soundId;unique.set(key,id);}
        clips[minute]=id;
        await this.protect([...Object.values(this.generated?.clips??{}),...Object.values(clips)]);
      }
      this.generated={signature,clips,verified:true};return clips[this.minute()];
    })().finally(()=>{this.pending=undefined;this.message.textContent='';this.sync();});
    this.sync();
    try{return await this.pending;}catch(error){await this.refresh();throw error;}
  }
}
