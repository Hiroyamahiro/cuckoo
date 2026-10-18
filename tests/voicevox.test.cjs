const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript');
const m={exports:{}};
new Function('require','module','exports',ts.transpileModule(fs.readFileSync('src/shared/voicevox.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,esModuleInterop:true,target:ts.ScriptTarget.ES2022}}).outputText)(()=>require('../src/shared/voicevox-legacy.json'),m,m.exports);
const {speechText,speechSignature,speechCredit,legacyVoicevoxCharacters:voicevoxCharacters}=m.exports;
const speech=(mode='time',character='zundamon')=>({character,mode,text:'そろそろ休憩するのだ！'});
test('credit preserves the explicit library wording rather than assuming its display name',()=>{
 const s={...speech(),character:'11111111-1111-4111-8111-111111111111',styleId:1,characterName:'もち子さん',policy:'「VOICEVOX:もち子(cv 明日葉よもぎ)」と記載してください。'};
 assert.equal(speechCredit(s),'VOICEVOX:もち子(cv 明日葉よもぎ)');assert.equal(speechCredit({...s,policy:undefined}),'もち子さん');assert.equal(speechCredit(speech()),'VOICEVOX:ずんだもん');
});
test('VOICEVOX time formatting covers all 1440 times and both characters',()=>{
 for(const c of voicevoxCharacters)for(let minute=0;minute<1440;minute++){
  const text=speechText(speech('time',c.id),minute);
  assert.equal(text,`${Math.floor(minute/60)}時${minute%60?`${minute%60}分`:''}です。`);
 }
 assert.equal(speechText(speech('time-text'),635),'10時35分です。そろそろ休憩するのだ！');
 assert.equal(speechText(speech('text'),0),'そろそろ休憩するのだ！');
});

test('dynamic UUID and style identity is independent of labels, gain and policy',()=>{
 const s={...speech(),character:'11111111-1111-4111-8111-111111111111',styleId:987,characterName:'New voice',styleName:'Soft',policy:'terms'};
 assert.equal(speechText(s,600),'10時です。');
 assert.notEqual(speechSignature(s,600),speechSignature({...s,styleId:988},600));
 assert.equal(speechSignature(s,600),speechSignature({...s,characterName:'Renamed',styleName:'New label',policy:'updated',volume:10,enabled:false},600));
 for(const styleId of [-1,1.5,4294967296,NaN])assert.throws(()=>speechText({...s,styleId},600));
 assert.throws(()=>speechText({...s,character:'not-a-uuid'},600));
});
test('VOICEVOX cache signatures reflect only spoken content and character',()=>{
 assert.equal(speechSignature(speech('text'),0),speechSignature(speech('text'),1439));
 assert.notEqual(speechSignature(speech('time'),0),speechSignature(speech('time'),1));
 assert.notEqual(speechSignature(speech(),0),speechSignature(speech('time','meimei-himari'),0));
 assert.equal(speechSignature(speech(),0),speechSignature({...speech(),text:'unused'},0));
});
test('VOICEVOX rejects empty phrases, oversized input and invalid modes or time',()=>{
 for(const s of [{...speech('text'),text:'  '},{...speech('text'),text:'あ'.repeat(501)},speech('unknown'),speech('time','missing'),{...speech('text'),text:'bad\0text'}])assert.throws(()=>speechText(s,0));
 for(const minute of [-1,1440,1.5])assert.throws(()=>speechText(speech(),minute));
});
