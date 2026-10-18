const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '..');

test('shared grant text preserves distinct copyrights and escapes source text', async () => {
  const { buildDocument } = await import('../scripts/generate-notices.mjs');
  const grant = 'Permission is hereby granted, free of charge, to use this software.\nTHE SOFTWARE IS PROVIDED AS IS.';
  const components = ['Alice', 'Bob'].map(author => ({ name: author, license: 'MIT', texts: [{ label: 'LICENSE', text: `MIT License\nCopyright (c) 2026 ${author}\n\n${grant}` }] }));
  components.push({ name: '<script>untrusted</script>', license: 'MIT', texts: [{ label: 'NOTICE', text: '<img src="https://example.invalid"> & notice' }] });
  const result = buildDocument(components, 'test');
  assert.equal([...result.pool.values()].filter(p => p.text.startsWith('Permission is hereby')).length, 1);
  assert.match(result.html, /Copyright \(c\) 2026 Alice/);
  assert.match(result.html, /Copyright \(c\) 2026 Bob/);
  assert.match(result.html, /&lt;script&gt;untrusted&lt;\/script&gt;/);
  assert.doesNotMatch(result.html, /<script|<img/);
  for (const c of result.entries) for (const r of c.references) assert.ok([...result.pool.values()].some(p => p.id === r.id));
});

test('offline notices have complete local references and no external resources', async () => {
  const { textKey } = await import('../scripts/generate-notices.mjs');
  const html = fs.readFileSync(path.join(root, 'src/public/THIRD-PARTY-NOTICES.html'), 'utf8');
  const ids = [...html.matchAll(/\bid="([^"]+)"/g)].map(m => m[1]);
  assert.equal(ids.length, new Set(ids).size);
  for (const m of html.matchAll(/href="#([^"]+)"/g)) assert.ok(ids.includes(m[1]), m[1]);
  assert.doesNotMatch(html, /<(?:script|link|img|iframe|object)\b|\bsrc=|@import|url\(/i);
  const bodies = [...html.matchAll(/class="license-text"><h3>[^]*?<pre>([^]*?)<\/pre>/g)].map(m => textKey(m[1]));
  assert.ok(bodies.length > 0);
  assert.equal(bodies.length, new Set(bodies).size);
  for (const name of ['Urbanist font', 'OtoLogic', 'Tabler Icons', 'Tauri JavaScript API', 'NSIS installer engine', 'NSIS Modern UI', 'Vite generated modulepreload helper', 'Tauri NSIS utilities plugin']) assert.ok(html.includes(name), name);
  for (const name of ['symphonia/0.5.5', 'symphonia-core/0.5.5', 'symphonia-metadata/0.5.5', 'symphonia-bundle-mp3/0.5.5']) assert.ok(html.includes(name + '/download'), name);
});

test('bundled Urbanist notices preserve the copyright and full OFL text',async()=>{
 const {escapeHtml}=await import('../scripts/generate-notices.mjs');
 const html=fs.readFileSync(path.join(root,'src/public/THIRD-PARTY-NOTICES.html'),'utf8').replace(/\r\n?/g,'\n');
 const text=fs.readFileSync(path.join(root,'src/fonts/urbanist/OFL.txt'),'utf8').replace(/\r\n?/g,'\n').trim();
 const escaped=escapeHtml(text);
 assert.ok(html.includes(escaped));
 assert.match(html,/Copyright 2021 The Urbanist Project Authors/);
});
