const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const source=fs.readFileSync(path.join(__dirname,'../src/settings-sync.ts'),'utf8');
const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const mod={exports:{}};new Function('module','exports',output)(mod,mod.exports);
test('new UI obtains current state; stale status cannot overwrite it',()=>{
 const sync=new mod.exports.SettingsSync();
 const status=(seq,revision,session='session-a')=>({settingsSync:{sessionId:session,eventSeq:seq,revision},settings:{volume:37}});
 const current=status(8,3);assert.equal(sync.accept(current),current);
 assert.equal(sync.accept(status(7,3)),current);
 assert.equal(sync.accept(status(2,1)),current);
 assert.equal(sync.accept(status(9,4,'old-session')),current);
 const saved=status(9,3);assert.equal(sync.accept(saved),saved);
 const recreated=new mod.exports.SettingsSync();assert.equal(recreated.accept(saved),saved);
});
test('audio and settings sequences advance independently without stale rollback',()=>{
 const sync=new mod.exports.SettingsSync();const status=(s,a,volume)=>({settingsSync:{sessionId:'same',eventSeq:s},settings:{volume},audio:{eventSeq:a,state:'Playing'},audioReady:true,message:'audio-'+a});
 sync.accept(status(8,3,37));let next=sync.accept(status(7,4,20));assert.equal(next.settings.volume,37);assert.equal(next.audio.eventSeq,4);
 next=sync.accept(status(9,2,38));assert.equal(next.settings.volume,38);assert.equal(next.audio.eventSeq,4);assert.equal(next.message,'audio-4');
 assert.equal(sync.accept(status(8,3,0)),next);
});
