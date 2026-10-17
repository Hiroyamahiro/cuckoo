const {test}=require('node:test'),assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
test('minimum display refits on DPI notification and cancels stale/unloaded measurements',async t=>{
 const previous={window:global.window,document:global.document,ResizeObserver:global.ResizeObserver};t.after(()=>{for(const[k,v]of Object.entries(previous))if(v===undefined)delete global[k];else global[k]=v});
 const events=new Map(),sent=[];const controls={offsetTop:500,hidden:false,inert:false};
 const nodes={'#normal-controls':controls,'#clock-mode':{setAttribute(){}},'#display-mode-hint':{},header:{getBoundingClientRect:()=>({bottom:100})}};
 global.document={querySelector:s=>nodes[s],documentElement:{dataset:{},style:{setProperty(){}}}};
 global.window={scrollY:0,scrollTo(){},addEventListener:(n,f)=>events.set(n,f),removeEventListener:(n)=>events.delete(n),matchMedia:()=>({matches:true,addEventListener(){}}),cuckoo:{fitMinimumDisplay:async h=>sent.push(h)}};
 global.ResizeObserver=class{observe(){}disconnect(){}};
 const output=ts.transpileModule(fs.readFileSync(path.join(__dirname,'../src/renderer/display-mode.ts'),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 const mod={exports:{}};new Function('module','exports',output)(mod,mod.exports);
 const mode=new mod.exports.DisplayMode({getBoundingClientRect:()=>({width:500,bottom:570})},assert.fail);
 mode.setMode(true);await Promise.resolve();assert.deepEqual(sent,[640]);assert(controls.hidden);
 events.get('native-display-refit')();await Promise.resolve();assert.deepEqual(sent,[640,640]);
 mode.setMode(false);mode.setMode(true);mode.setMode(false);await Promise.resolve();assert.deepEqual(sent,[640,640]);assert(!controls.hidden);
 mode.setMode(true);events.get('unload')();await Promise.resolve();assert.deepEqual(sent,[640,640]);assert(!events.has('native-display-refit'));
});
