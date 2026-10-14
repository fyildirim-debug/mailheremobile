import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { cargoWorkspaceVersion, check, versionCodeMatches, versionCodeOf } from './surum-denetle.mjs';

const cargo = '[workspace]\nmembers = ["src-tauri"]\n\n[workspace.package]\nversion = "0.3.1"\nedition = "2021"\n\n[profile.release]\nlto = true\n';

test('versionCode şeması', () => {
  assert.equal(versionCodeOf('0.3.1'), 30100);
  assert.equal(versionCodeOf('0.3.1', 7), 30107);
  assert.equal(versionCodeOf('1.12.9'), 1_120_900);
  assert.ok(versionCodeMatches('0.3.1', 30100));
  assert.ok(versionCodeMatches('0.3.1', 30199));
  assert.ok(!versionCodeMatches('0.3.1', 30200));
  assert.ok(!versionCodeMatches('0.3.1', 3001));
  assert.throws(() => versionCodeOf('0.100.0'));
});

test('Cargo.toml sürümü okunur', () => {
  assert.equal(cargoWorkspaceVersion(cargo), '0.3.1');
});

test('tutarlı sürüm hata vermez, uyuşmazlık yakalanır', () => {
  const ok = {
    pkg: { version: '0.3.1' },
    conf: { version: '0.3.1', bundle: { android: { versionCode: 30100 } } },
    cargo,
    corePkg: { version: '0.3.1' },
    etiket: 'v0.3.1',
  };
  assert.deepEqual(check(ok), []);
  assert.equal(check({ ...ok, etiket: 'v0.3.2' }).length, 1);
  assert.equal(check({ ...ok, etiket: '0.3.1' }).length, 1);
  assert.equal(check({ ...ok, corePkg: { version: '0.4.0' } }).length, 1);
  assert.equal(check({ ...ok, conf: { version: '0.3.1', bundle: { android: { versionCode: 3001 } } } }).length, 1);
});

test('depodaki sürümler tutarlı', () => {
  const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8');
  const errors = check({
    pkg: JSON.parse(read('package.json')),
    conf: JSON.parse(read('src-tauri/tauri.conf.json')),
    cargo: read('Cargo.toml'),
    corePkg: JSON.parse(read('mailhere/package.json')),
  });
  assert.deepEqual(errors, []);
});
