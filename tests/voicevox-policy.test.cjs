const {test}=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript');
const legacy=require('../src/shared/voicevox-legacy.json'),m={exports:{}};
new Function('require','module','exports',ts.transpileModule(fs.readFileSync('src/renderer/voicevox-policy.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText)(()=>({legacyVoicevoxCharacters:legacy}),m,m.exports);
const {voicevoxPolicyDisplay,renderVoicevoxPolicy}=m.exports;
test('displays the explicit formal credit and supplied terms URL without descriptions',()=>{
 assert.deepEqual(voicevoxPolicyDisplay({character:'uuid',policy:'「VOICEVOX:もち子(cv 明日葉よもぎ)」と記載。利用規約 https://example.com/terms'}),{credit:'VOICEVOX:もち子(cv 明日葉よもぎ)',url:'https://example.com/terms'});
 assert.deepEqual(voicevoxPolicyDisplay({character:'unknown',policy:''}),{credit:'',url:null});
 assert.equal(voicevoxPolicyDisplay({character:'unknown',policy:'https://a.example/ https://b.example/'}).url,null);
 assert.equal(voicevoxPolicyDisplay({character:'unknown',policy:'javascript:alert(1) https://user:pass@example.com/'}).url,null);
 assert.equal(voicevoxPolicyDisplay({character:'zundamon'}).credit,'VOICEVOX:ずんだもん');
});
test('renders only two items and opens the terms through the existing external-browser bridge',async()=>{
 class Element{children=[];textContent='';listeners={};append(...nodes){this.children.push(...nodes);}replaceChildren(...nodes){this.children=nodes;}addEventListener(type,fn){this.listeners[type]=fn;}}
 const savedDocument=global.document,savedWindow=global.window;let opened;
 global.document={createElement:()=>new Element()};global.window={cuckoo:{openCredit:async link=>{opened=link;}}};
 try{
  const host=new Element();renderVoicevoxPolicy(host,{character:'uuid',policy:'説明文「VOICEVOX:テスト」詳細 https://example.com/terms'});
  assert.equal(host.children.length,2);assert.equal(host.children[0].textContent,'VOICEVOX:テスト');
  const link=host.children[1].children[0];assert.equal(link.textContent,'利用規約');assert.equal(link.href,'https://example.com/terms');
  let prevented=false;link.listeners.click({preventDefault(){prevented=true;}});await Promise.resolve();
  assert.equal(prevented,true);assert.equal(opened,'voicevox-policy:https://example.com/terms');
  renderVoicevoxPolicy(host,{character:'unknown'});assert.equal(host.children.length,0);
 }finally{global.document=savedDocument;global.window=savedWindow;}
});
test('accordion defaults to collapsed and resets when opening an editor',()=>{
 const html=fs.readFileSync('src/index.html','utf8');assert.match(html,/<details id="voicevox-policy">/);assert.doesNotMatch(html,/id="voicevox-policy-(?:status|text)"/);
 assert.match(fs.readFileSync('src/renderer/voicevox-controls.ts','utf8'),/getElementById\('voicevox-policy'\).*\.open=false/);
});
