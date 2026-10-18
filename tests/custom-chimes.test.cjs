const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs'),path=require('node:path'),ts=require('typescript');
const source=fs.readFileSync(path.join(__dirname,'../src/custom-chimes.ts'),'utf8');
const js=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
const m={exports:{}};new Function('module','exports',js)(m,m.exports);
const {chimeTime,parseChimeTime,putChime,customMinutes,toggleCustomMinute,customMinuteTooltip,stepTimePart,customMinutesOnArc,setCustomMinutes}=m.exports;
const chime=(id,minute,enabled=true)=>({id,minute,enabled,soundId:'builtin:cuckoo-clock',repeat:1,volume:80});
test('all 1440 minute values roundtrip and invalid times are rejected',()=>{
 for(let minute=0;minute<1440;minute++)assert.equal(parseChimeTime(chimeTime(minute)),minute);
 for(const time of ['24:00','12:60','9:00','12:00:00','','-1:00'])assert.throws(()=>parseChimeTime(time));
});
test('custom chimes sort, edit in place and reject duplicates including disabled entries',()=>{
 const original=[chime('a',600),chime('b',2,false)];
 const added=putChime(original,chime('c',1439));assert.deepEqual(added.map(c=>c.minute),[2,600,1439]);
 assert.deepEqual(original.map(c=>c.minute),[600,2]);
 const edited=putChime(added,{...added[1],minute:0,volume:25});assert.equal(edited.length,3);assert.equal(edited[0].volume,25);
 assert.throws(()=>putChime(added,chime('d',2)),/同じ時刻/);
});
test('custom markers use absolute minutes on the 24 hour dial',()=>{
 assert.equal(customMinutes([]).size,0);assert.equal(customMinutes([chime('a',3,false)]).size,0);
 assert.deepEqual([...customMinutes([chime('a',3),chime('b',63),chime('c',1439)])],[3,63,1439]);
});
test('dial toggles only the selected time and tooltip preserves its time and state',()=>{
 const original=[chime('a',63),chime('b',123,false),chime('c',64)];
 const off=toggleCustomMinute(original,63);assert.deepEqual(off.map(c=>c.enabled),[false,false,true]);
 const on=toggleCustomMinute(off,63);assert.deepEqual(on.map(c=>c.enabled),[true,false,true]);
 assert.deepEqual(original.map(c=>c.enabled),[true,false,true]);
 assert.equal(customMinuteTooltip(original,63),'01:03  ON');
 assert.equal(customMinuteTooltip(original,123),'02:03  OFF');
});
test('time editor wraps each segment independently in both directions',()=>{
 assert.equal(stepTimePart(23,1,24),0);assert.equal(stepTimePart(0,-1,24),23);
 assert.equal(stepTimePart(59,1,60),0);assert.equal(stepTimePart(0,-1,60),59);
 assert.equal(stepTimePart(10,-3,60),7);
});

test('custom drag selects registered times across midnight in both directions',()=>{
 const minutes=[0,1,10,600,1438,1439];
 assert.deepEqual(customMinutesOnArc(minutes,1438,1),[0,1,1438,1439]);
 assert.deepEqual(customMinutesOnArc(minutes,1,1438),[0,1,1438,1439]);
 assert.deepEqual(customMinutesOnArc(minutes,10,10),[10]);
 assert.deepEqual(customMinutesOnArc(minutes,10,600),[10,600]);
});
test('custom drag replaces enabled state once without altering chime details',()=>{
 const original=[chime('a',0,false),chime('b',1,true),chime('c',600,false)];
 const on=setCustomMinutes(original,[0,1],true);
 assert.deepEqual(on.map(c=>c.enabled),[true,true,false]);
 assert.deepEqual(setCustomMinutes(on,[0,1],false).map(c=>c.enabled),[false,false,false]);
 assert.equal(on[2],original[2]);
 assert.deepEqual(on[0],{...original[0],enabled:true});
 assert.deepEqual(original.map(c=>c.enabled),[false,true,false]);
});
