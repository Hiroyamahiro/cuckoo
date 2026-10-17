const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const source=fs.readFileSync(path.join(__dirname,'../src/preset-client.ts'),'utf8');
const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const mod={exports:{}};new Function('module','exports',output)(mod,mod.exports);const {PresetClient}=mod.exports;
const status={settingsSync:{sessionId:'session',revision:3}};
const create={type:'create',name:'Morning'};
function client(responder){const requests=[];return {requests,client:new PresetClient(async()=>status,async r=>{requests.push(structuredClone(r));return {requestId:r.requestId,status,...await responder(requests.length,r)}},s=>s)}}
test('preset returns success only after a saved receipt',async()=>{
 const x=client(async()=>({operation:{state:'saved',id:'id-one'}}));assert.equal((await x.client.edit(create)).id,'id-one');assert.equal(x.requests[0].baseRevision,3);
});
test('timeout retries the same request without a second create',async()=>{
 const x=client(async n=>({operation:n===1?{state:'pending'}:{state:'saved',id:'once'}}));await assert.rejects(x.client.edit(create),/未確定/);assert.equal((await x.client.edit(create)).id,'once');assert.equal(x.requests[0].requestId,x.requests[1].requestId);
});
test('known failed save is an error; explicit retry gets a new request',async()=>{
 const x=client(async n=>({operation:n===1?{state:'failed',code:'disk denied',uncertain:false}:{state:'saved',id:'new'}}));await assert.rejects(x.client.edit(create),/反映していません/);await x.client.edit(create);assert.notEqual(x.requests[0].requestId,x.requests[1].requestId);
});
test('transport loss preserves operation identity',async()=>{
 const x=client(async n=>{if(n===1)throw Error('transport lost');return {operation:{state:'saved',id:'once'}}});await assert.rejects(x.client.edit(create));await x.client.edit(create);assert.equal(x.requests[0].requestId,x.requests[1].requestId);
});
test('uncertain commit cannot be relabeled as success or replaced by another operation',async()=>{
 const x=client(async()=>({operation:{state:'failed',uncertain:true,code:'commit uncertain'}}));await assert.rejects(x.client.edit(create),/未確定/);await assert.rejects(x.client.edit({type:'delete',id:'old'}),/未確定/);assert.equal(x.requests[0].requestId,x.requests[1].requestId);assert.equal(x.requests[1].action.type,'create');
});
test('new action resolves an older pending receipt before submitting a fresh action',async()=>{
 const x=client(async n=>({operation:n===1?{state:'pending'}:{state:'saved',id:'one'}}));await assert.rejects(x.client.edit(create));await x.client.edit({type:'rename',id:'one',name:'Noon'});assert.equal(x.requests[0].requestId,x.requests[1].requestId);assert.notEqual(x.requests[1].requestId,x.requests[2].requestId);
});
