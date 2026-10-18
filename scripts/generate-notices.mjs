import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
process.chdir(root);
const target = 'x86_64-pc-windows-msvc';
const manifest = 'src-tauri/Cargo.toml';
const output = 'src/public/THIRD-PARTY-NOTICES.html';
const supplemental = JSON.parse(fs.readFileSync('licenses/supplemental.json', 'utf8'));
const cargo = args => execFileSync('cargo', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
const hash = text => createHash('sha256').update(text).digest('hex');
const normalize = text => text.replace(/\r\n?/g, '\n').trim();
// Only whitespace is ignored. Copyright holders, dates and every other character
// remain part of the identity; SPDX names alone are never deduplication keys.
export const textKey = text => normalize(text).replace(/\s+/g, ' ');
export const escapeHtml = text => String(text).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

function walk(dir, prefix = '') {
  return fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, 'en')).flatMap(entry => {
    const relative = prefix + entry.name;
    if (entry.isSymbolicLink()) throw new Error('Unexpected symlink: ' + relative);
    if (entry.isDirectory()) {
      if (/^(\.git|target|tests?|benches|examples|node_modules)$/.test(entry.name)) return [];
      return walk(path.join(dir, entry.name), relative + '/');
    }
    return [relative];
  });
}

function collect(pkg) {
  const dir = path.dirname(pkg.manifest_path);
  const files = walk(dir);
  const licenseFiles = files.filter(f => /^(licen[cs]e|copying|notice|copyright|authors)([-._]|$)/i.test(path.basename(f)) || /(^|\/)licen[cs]es\//i.test(f));
  if (pkg.license_file && !licenseFiles.includes(pkg.license_file)) licenseFiles.push(pkg.license_file);
  const texts = licenseFiles.sort().map(f => ({ label: f, text: fs.readFileSync(path.join(dir, f), 'utf8') }));
  // Preserve copyright statements carried in compiled source files as well as
  // the license files. Cargo's authors field is supplementary, not a substitute.
  const copyrights = new Set();
  for (const f of files.filter(f => /\.(rs|c|h|cpp|js|ts)$/.test(f))) {
    for (const line of fs.readFileSync(path.join(dir, f), 'utf8').split(/\r?\n/)) {
      if (/copyright.*\d{4}/i.test(line) && /^\s*(?:\/\/|\/\*|\*|#)/.test(line)) copyrights.add(line.trim());
    }
  }
  const key = pkg.name + ' ' + pkg.version;
  const extra = supplemental.rustSupplements[key];
  if (extra && extra.license !== pkg.license) throw new Error('Supplement license changed: ' + key);
  if (extra) extra.texts.forEach((text, i) => texts.push({ label: 'Supplement ' + (i + 1), text }));
  if (!texts.length && !extra) throw new Error('Unreviewed missing license file: ' + key);
  for (const id of ['MPL-2.0', 'Apache-2.0']) {
    if (pkg.license?.includes(id) && !texts.some(t => id === 'MPL-2.0' ? /Mozilla Public License\s+Version 2\.0/.test(t.text) : /TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION/.test(t.text))) {
      texts.push({ label: id + ' full text', text: supplemental.shared[id] });
    }
  }
  if (!texts.length || !pkg.license) throw new Error('Missing license evidence: ' + key);
  return { name: key, license: pkg.license, source: `https://crates.io/api/v1/crates/${pkg.name}/${pkg.version}/download`, authors: pkg.authors.join(', '), copyrights: [...copyrights].sort(), texts, rust: true };
}

export function buildDocument(components, fingerprint) {
  const pool = new Map();
  const intern = (label, raw) => {
    const text = normalize(raw);
    if (!text || text.includes('\uFFFD')) throw new Error('Empty or corrupt notice: ' + label);
    const key = textKey(text);
    if (!pool.has(key)) pool.set(key, { id: 'text-' + hash(key).slice(0, 20), text, labels: new Set() });
    pool.get(key).labels.add(label);
    return pool.get(key).id;
  };
  const entries = components.map((component, index) => {
    const references = [];
    for (const { label, text } of component.texts) {
      // MIT copyright headers stay attached to the component; identical grant
      // and disclaimer bodies are shared without editing their wording.
      const marker = text.indexOf('Permission is hereby granted, free of charge,');
      const parts = marker > 0 && /\bMIT\b/.test(component.license) && /MIT|copyright/i.test(text.slice(0, marker))
        ? [text.slice(0, marker), text.slice(marker)] : [text];
      for (const part of parts) {
        const id = intern(label, part);
        if (!references.some(r => r.id === id)) references.push({ id, label });
      }
    }
    return { ...component, id: 'component-' + index, references };
  });
  const esc = escapeHtml;
  const body = entries.map(c => `<article id="${c.id}" data-kind="${c.rust ? 'rust' : 'other'}"><h3>${esc(c.name)}</h3><p>License declaration: ${esc(c.license)}</p>${c.authors ? `<p>Authors: ${esc(c.authors)}</p>` : ''}${c.source ? `<p>Source: ${/^https:\/\//.test(c.source) ? `<a href="${esc(c.source)}">${esc(c.source)}</a>` : esc(c.source)}</p>` : ''}${c.copyrights?.length ? `<pre class="copyrights">${esc(c.copyrights.join('\n'))}</pre>` : ''}<ul>${c.references.map(r => `<li><a href="#${r.id}">${esc(r.label)}</a></li>`).join('')}</ul></article>`).join('\n');
  const blocks = [...pool.values()].map(t => `<section id="${t.id}" class="license-text"><h3>${esc([...t.labels].join(' / '))}</h3><pre>${esc(t.text)}</pre><p><a href="#components">Component list</a></p></section>`).join('\n');
  const html = `<!doctype html>
<html lang="ja"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="generator" content="Cuckoo generate-notices.mjs"><meta name="inputs-sha256" content="${fingerprint}"><title>Cuckoo — Third-party notices</title>
<style>:root{color-scheme:light dark;font-family:system-ui,sans-serif}body{margin:0}main{max-width:1000px;margin:auto;padding:24px}h1{font-size:24px}article,.license-text{border-top:1px solid GrayText;margin-top:24px;padding-top:12px}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.6}a{overflow-wrap:anywhere}:target{outline:2px solid GrayText;outline-offset:4px}nav{display:flex;gap:20px;flex-wrap:wrap}@media print{a{color:inherit}}</style></head>
<body><main><h1>Cuckoo — 第三者ライセンス / Third-party notices</h1>
<p>Windows x64 production build. Rust components: ${entries.filter(c => c.rust).length}; other components: ${entries.filter(c => !c.rust).length}; shared text blocks: ${pool.size}.</p>
<nav><a href="#components">コンポーネント一覧</a><a href="#license-texts">ライセンス本文</a><a href="#source-code">ソース入手案内</a></nav>
<section id="source-code"><h2>ソース入手案内 / Source availability</h2><p>The Rust components listed below are used without source modifications. Exact-version source archives, including the MPL-2.0 covered components, are available using each component's Source link. MPL-covered source is available under MPL-2.0; compiling it does not change its license. The links are for obtaining source; no network access is needed to read this document or its license texts.</p></section>
<section id="components"><h2>コンポーネント一覧 / Components</h2>${body}</section>
<section id="license-texts"><h2>ライセンス本文・著作権・NOTICE / License texts and notices</h2>${blocks}</section></main></body></html>\n`;
  return { html, entries, pool };
}

export function generate() {
  const metadata = JSON.parse(cargo(['metadata', '--offline', '--locked', '--format-version', '1', '--filter-platform', target, '--manifest-path', manifest]));
  // cargo tree resolves active target/features separately for host build tools.
  // Walking metadata's unified node edges alone over-includes their dependencies.
  const tree = cargo(['tree', '--offline', '--locked', '--manifest-path', manifest, '--target', target, '--features', 'tauri/custom-protocol', '--edges', 'normal,no-proc-macro', '--prefix', 'none', '--format', '{p}', '--no-dedupe']);
  const selected = new Set(tree.trim().split(/\r?\n/).map(line => line.replace(' v', ' ')));
  const packages = metadata.packages.filter(p => p.id !== metadata.resolve.root && selected.has(p.name + ' ' + p.version));
  if (packages.length !== selected.size - 1) throw new Error('Ambiguous Cargo package selection');
  const components = packages.sort((a, b) => (a.name + a.version).localeCompare(b.name + b.version, 'en')).map(collect);
  for (const c of supplemental.components) {
    let version = '';
    const texts = (c.texts || []).map(text => ({ label: c.license, text }));
    if (c.file) texts.push({ label: c.file, text: fs.readFileSync(c.file, 'utf8') });
    if (c.package) {
      const dir = 'node_modules/' + c.package;
      const p = JSON.parse(fs.readFileSync(dir + '/package.json', 'utf8'));
      const lock = JSON.parse(fs.readFileSync('package-lock.json', 'utf8')).packages[dir];
      if (p.version !== lock.version || p.license !== lock.license) throw new Error('npm package/lock mismatch');
      version = ' ' + p.version;
      for (const f of c.files) {
        const raw = fs.readFileSync(dir + '/' + f, 'utf8');
        if (c.before && !raw.includes(c.before)) throw new Error('Missing license boundary: ' + c.package);
        texts.push({ label: f, text: c.before ? raw.split(c.before)[0] : raw });
      }
    }
    components.push({ ...c, name: c.name + version, texts });
  }
  const fingerprint = hash(['src-tauri/Cargo.lock', manifest, 'package-lock.json', 'licenses/supplemental.json', 'src-tauri/tauri.conf.json'].map(f => fs.readFileSync(f, 'utf8').replace(/\r\n?/g, '\n')).join('\n'));
  const result = buildDocument(components, fingerprint);
  if (process.argv.includes('--check')) {
    if (!fs.existsSync(output) || fs.readFileSync(output, 'utf8').replace(/\r\n?/g, '\n') !== result.html) throw new Error('Notices are stale; run npm run licenses:generate');
  } else {
    fs.mkdirSync(path.dirname(output), { recursive: true });
    fs.writeFileSync(output, result.html);
  }
  console.log(`${output}: ${packages.length} Rust + ${components.length - packages.length} other components, ${result.pool.size} unique text blocks, ${Buffer.byteLength(result.html)} bytes`);
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) generate();
