const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript');
const js=ts.transpileModule(fs.readFileSync('src/audio-client.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const m={exports:{}};new Function('module','exports',js)(m,m.exports);const {waitForPlayback}=m.exports;
test('audio acceptance waits for actual terminal completion',async()=>{let reads=0;await waitForPlayback(7,async id=>{assert.equal(id,7);return {id,phase:'Playing',outcome:++reads<3?null:{state:'Completed'}}},async()=>{});assert.equal(reads,3)});
for(const state of ['Interrupted','Failed','Skipped'])test('audio '+state+' is not completion success',async()=>{await assert.rejects(waitForPlayback(3,async()=>({id:3,phase:'Finished',outcome:{state,code:'Injected'}})),/Injected/)});
test('Stuck and missing receipts never claim safe completion',async()=>{await assert.rejects(waitForPlayback(1,async()=>({id:1,phase:'Stuck',outcome:null})),/停止を確認/);await assert.rejects(waitForPlayback(1,async()=>null),/結果を確認/)});
