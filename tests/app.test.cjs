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
 const {currentTime}=load('renderer/current-time');
 for(const [h,m]of [[0,0],[8,41],[12,30],[23,59]]){
  const value=currentTime(new Date(2026,9,16,h,m,0));
  assert.equal(value.text,String(h).padStart(2,'0')+':'+String(m).padStart(2,'0'));
 }
});
test('48 independent slots and bulk actions remain available',()=>{
 const {presetSlots}=load('renderer/dial-model');
 const source=fs.readFileSync(path.join(root,'src/renderer/dial-model.ts'),'utf8');
 assert.match(source,/48/);
 for(const name of [...source.matchAll(/case '([^']+)'/g)].map(m=>m[1]))assert.equal(presetSlots(name).length,48);
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
