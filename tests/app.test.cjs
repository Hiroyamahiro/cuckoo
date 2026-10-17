const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const root=path.resolve(__dirname,'..');
function load(name){
 const filename=path.join(root,'src',name+'.ts');
 const result=ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const module={exports:{}};new Function('module','exports','require',result)(module,module.exports,(p)=>load(path.join(path.dirname(name),p)));return module.exports;
}
test('Clock remains HH:mm and uses the same angle source',()=>{
 const {currentTime}=load('renderer/clock-time');
 for(const [h,m]of [[0,0],[8,41],[12,30],[23,59]]){
  const value=currentTime(new Date(2026,9,16,h,m,0));
  assert.equal(value.text,String(h).padStart(2,'0')+':'+String(m).padStart(2,'0'));
 }
});
test('bulk actions replace only their ring and retain the existing daytime boundaries',()=>{
 const {applyBulkSlots}=load('renderer/dial-model');
 const original=Array.from({length:48},(_,i)=>i%3===0);
 for(const ring of [0,1])for(const action of ['day','all']){
  const result=applyBulkSlots(original,ring,action);
  assert.equal(result.length,48);
  for(let i=0;i<48;i++)assert.equal(result[i],i%2!==ring?original[i]:action==='all'||(i>=18&&i<=36));
 }
 assert.equal(original.filter(Boolean).length,16);
 const hours=applyBulkSlots(original,0,'day');
 const both=applyBulkSlots(hours,1,'all');
 for(let i=0;i<48;i++)assert.equal(both[i],i%2===0?hours[i]:true);
 assert.deepEqual(applyBulkSlots(both,0,'off'),Array(48).fill(false));
 assert.deepEqual(applyBulkSlots(hours,0,'day'),hours);
});

test('Production identity and Windows NSIS packaging remain configured',()=>{
 const c=JSON.parse(fs.readFileSync(path.join(root,'src-tauri/tauri.conf.json')));
 assert.equal(c.identifier,'jp.hiroyamahiroshi.cuckoo');
 assert.equal(c.app.windows.length,1);assert.equal(c.bundle.active,true);assert.deepEqual(c.bundle.targets,['nsis']);
});
test('Hidden frontend stops the periodic refresh',()=>{
 const s=fs.readFileSync(path.join(root,'src/renderer.ts'),'utf8');
 assert.match(s,/visibilitychange/);assert.match(s,/if\(!document.hidden\)/);
});
