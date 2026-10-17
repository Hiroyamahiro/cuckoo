const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const ts = require('typescript');
const moduleUnderTest = { exports: {} };
new Function('module', 'exports', ts.transpileModule(fs.readFileSync(require('node:path').join(__dirname, '../src/update-menu.ts'), 'utf8'), { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 } }).outputText)(moduleUnderTest, moduleUnderTest.exports);
const { updateMenu } = moduleUnderTest.exports;
const base = { seq: 0, phase: 'idle', version: null, received: 0, total: 0, message: '' };

test('only an available stable version shows the update dot and install action', () => {
  assert.deepEqual(updateMenu(base), { dot: false, disabled: false, cancel: false, label: '更新を確認', action: 'check', message: '' });
  const available = updateMenu({ ...base, phase: 'available', version: '2.0.10' });
  assert.equal(available.label, '更新する v2.0.10');
  assert.equal(available.dot, true);
  assert.equal(available.action, 'install');
});

test('progress and recoverable errors stay within the menu', () => {
  const view = updateMenu({ ...base, phase: 'downloading', version: '2.0.2', received: 42, total: 100 });
  assert.equal(view.label, 'ダウンロード中 42%');
  assert.equal(view.disabled, true);
  assert.equal(view.cancel, true);
  const failed = updateMenu({ ...base, phase: 'available', version: '2.0.2', message: '照合に失敗しました' });
  assert.equal(failed.disabled, false);
  assert.equal(failed.action, 'install');
  assert.equal(failed.message, '照合に失敗しました');
  assert.equal(updateMenu({ ...base, phase: 'checking' }).disabled, true);
  assert.equal(updateMenu({ ...base, phase: 'launched' }).disabled, true);
});
