import characters from './voicevox-legacy.json';
// These aliases exist only to read pre-dynamic settings and keep their cache keys.
export {characters as legacyVoicevoxCharacters};
export interface Speech {character:string;styleId?:number;characterName?:string;styleName?:string;policy?:string;mode:'time'|'time-text'|'text';text:string}
export interface GeneratedSpeech {soundId:string;cached:boolean}
export interface VoicevoxStyle {id:number;name:string;type:string}
export interface VoicevoxCharacter {uuid:string;name:string;styles:VoicevoxStyle[];version:string}
export interface VoicevoxConnection {version:string;characters:VoicevoxCharacter[]}
export function speechName(s:Speech):string {return `VOICEVOX:${s.characterName??characters.find(c=>c.id===s.character)?.name??s.character}${s.styleName?`（${s.styleName}）`:''}`;}
// Display an explicit credit quoted by the library; do not invent a legal credit
// from an unfamiliar character's display name or infer permission from it.
export function speechCredit(s:Speech):string {
  const quoted=s.policy?.match(/[「｢“"]\s*(VOICEVOX[:：][^\r\n」｣”"]{1,200})[」｣”"]/)?.[1]?.trim();
  return quoted??characters.find(c=>c.id===s.character)?.credit??s.characterName??s.character;
}
export function validVoice(s:Speech):boolean{return s.styleId===undefined?characters.some(c=>c.id===s.character):/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(s.character)&&Number.isSafeInteger(s.styleId)&&s.styleId>=0&&s.styleId<=4294967295;}
export function speechText(s:Speech,minute:number):string {
  if(!Number.isInteger(minute)||minute<0||minute>=1440||!validVoice(s)||!['time','time-text','text'].includes(s.mode)||[...s.text].length>500||[...s.text].some(c=>(c.charCodeAt(0)<32&&!['\n','\t'].includes(c))||c.charCodeAt(0)===127))throw new Error('読み上げ設定を確認してください。文章は500文字以内で入力してください。');
  const time=`${Math.floor(minute/60)}時${minute%60?`${minute%60}分`:''}です。`;
  if(s.mode==='time')return time;
  if(!s.text.trim())throw new Error('自由セリフを入力してください。');
  return(s.mode==='time-text'?time:'')+s.text.trim();
}
export function speechSignature(s:Speech,minute:number):string{return JSON.stringify([s.character,...(s.styleId===undefined?[]:[s.styleId]),speechText(s,minute)]);}
