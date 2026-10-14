import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { compare, parseLock, sharedNames } from './kilit-denetle.mjs';

const lock = (pairs) =>
  pairs.map(([n, v]) => `[[package]]\nname = "${n}"\nversion = "${v}"\n`).join('\n');

test('kilit ayrıştırılır, aynı paketin birden çok sürümü toplanır', () => {
  const m = parseLock(lock([['tauri', '2.12.3'], ['jni', '0.21.1'], ['jni', '0.22.4']]));
  assert.deepEqual([...m.get('tauri')], ['2.12.3']);
  assert.deepEqual([...m.get('jni')].sort(), ['0.21.1', '0.22.4']);
});

test('yalnız iki kilitte de bulunan ortak paketler karşılaştırılır', () => {
  const mobil = parseLock(lock([['tauri', '2.12.3'], ['tauri-plugin-fs', '2.7.0'], ['tracing-android', '0.2.0']]));
  const cekirdek = parseLock(lock([['tauri', '2.12.3'], ['tauri-plugin-fs', '2.7.0'], ['tauri-plugin-updater', '2.13.3']]));
  assert.deepEqual(compare(mobil, cekirdek), []);
  assert.ok(sharedNames(mobil, cekirdek).includes('tauri-plugin-fs'));
  assert.ok(!sharedNames(mobil, cekirdek).includes('tauri-plugin-updater'));
});

test('sürüm farkı yakalanır', () => {
  const mobil = parseLock(lock([['rusqlite', '0.40.2'], ['tauri-plugin-dialog', '2.8.2']]));
  const cekirdek = parseLock(lock([['rusqlite', '0.40.1'], ['tauri-plugin-dialog', '2.8.2']]));
  assert.deepEqual(compare(mobil, cekirdek), [
    { name: 'rusqlite', mobil: ['0.40.2'], cekirdek: ['0.40.1'] },
  ]);
});

test('depodaki iki kilit uyumlu', () => {
  const mobil = parseLock(readFileSync(new URL('../Cargo.lock', import.meta.url), 'utf8'));
  const cekirdek = parseLock(readFileSync(new URL('../mailhere/Cargo.lock', import.meta.url), 'utf8'));
  assert.deepEqual(compare(mobil, cekirdek), []);
});
