const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript');
function load(api){const module={exports:{}};const source=ts.transpileModule(fs.readFileSync('src/renderer/voicevox-thumbnails.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;new Function('module','exports','window',source)(module,module.exports,{cuckoo:{voicevoxThumbnail:api}});return module.exports.voicevoxThumbnail;}
const tick=()=>new Promise(r=>setImmediate(r));
test('thumbnails share in-flight and completed requests and separate Engine versions/styles',async()=>{
 let calls=0;const get=load(async()=>{calls++;return 'data:image/png;base64,test';});
 const [a,b]=await Promise.all([get('uuid',1,'v1'),get('uuid',1,'v1')]);assert.equal(a,b);assert.equal(calls,1);
 await get('uuid',1,'v1');assert.equal(calls,1);await get('uuid',2,'v1');await get('uuid',1,'v2');assert.equal(calls,3);
});
test('thumbnail network concurrency is bounded and all queued requests complete',async()=>{
 let active=0,max=0;const pending=[];const get=load(()=>new Promise(resolve=>{active++;max=Math.max(max,active);pending.push(()=>{active--;resolve('image');});}));
 const jobs=Array.from({length:9},(_,i)=>get('uuid',i,'v1'));await tick();assert.equal(active,3);
 for(let i=0;i<9;i++){assert.ok(pending.length);pending.shift()();await tick();}await Promise.all(jobs);assert.equal(max,3);assert.equal(active,0);
});
test('synchronous transport failures release all network slots',async()=>{
 const get=load(()=>{throw Error('unsupported');});assert.deepEqual(await Promise.all(Array.from({length:8},(_,i)=>get('uuid',i,'v1'))),Array(8).fill(null));
});
test('failure is cached briefly without rejecting selection and can retry later',async()=>{
 const now=Date.now;let time=now(),calls=0;Date.now=()=>time;
 try{const get=load(async()=>{calls++;if(calls===1)throw Error('offline');return 'image';});assert.equal(await get('uuid',1,'v1'),null);assert.equal(await get('uuid',1,'v1'),null);assert.equal(calls,1);time+=30001;assert.equal(await get('uuid',1,'v1'),'image');assert.equal(calls,2);}finally{Date.now=now;}
});
test('completed image cache evicts old data when its memory budget is reached',async()=>{
 let calls=0;const get=load(async()=>{calls++;return 'x'.repeat(600000);});for(let i=0;i<9;i++)await get('uuid',i,'v1');assert.equal(calls,9);await get('uuid',0,'v1');assert.equal(calls,10);
});
