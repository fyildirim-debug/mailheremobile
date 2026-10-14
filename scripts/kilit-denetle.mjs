#!/usr/bin/env node
// İki Cargo.lock'ta (bu depo ve mailhere alt modülü) ortak paketlerin sürümleri
// aynı mı? Kilitler ayrı olduğundan zamanla sürüklenebilir; ortak çekirdek iki
// kabukta da aynı tauri / SQLCipher / TLS sürümleriyle derlensin diye CI bu
// betiği çalıştırır. Fark varsa çıkış kodu 1.
//
//   node scripts/kilit-denetle.mjs [--mobil Cargo.lock] [--cekirdek mailhere/Cargo.lock]

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/** Her iki kilitte de bulunduğunda sürümü aynı olması gereken paketler. */
export const ORTAK = [
  'tauri',
  'tauri-build',
  'tauri-utils',
  'tauri-runtime',
  'tauri-runtime-wry',
  'tauri-plugin',
  'wry',
  'tao',
  'rusqlite',
  'libsqlite3-sys',
  'openssl-src',
  'ring',
  'rustls',
  'tokio',
];

/** `Cargo.lock` metni → paket adı → sürüm kümesi. */
export function parseLock(text) {
  const map = new Map();
  let name = null;
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (line === '[[package]]') {
      name = null;
      continue;
    }
    const n = /^name = "([^"]+)"$/.exec(line);
    if (n) {
      name = n[1];
      if (!map.has(name)) map.set(name, new Set());
      continue;
    }
    const v = /^version = "([^"]+)"$/.exec(line);
    if (v && name) {
      map.get(name).add(v[1]);
    }
  }
  return map;
}

/** Ortak paketler: sabit liste + iki kilitte de bulunan tüm `tauri-plugin-*`. */
export function sharedNames(mobil, cekirdek) {
  const names = new Set(ORTAK);
  for (const name of mobil.keys()) {
    if (name.startsWith('tauri-plugin-') && cekirdek.has(name)) names.add(name);
  }
  return [...names].sort();
}

/** Sürümü uyuşmayan paketler. Yalnız bir kilitte olan paket sayılmaz. */
export function compare(mobil, cekirdek) {
  const out = [];
  for (const name of sharedNames(mobil, cekirdek)) {
    const a = mobil.get(name);
    const b = cekirdek.get(name);
    if (!a || !b) continue;
    const av = [...a].sort();
    const bv = [...b].sort();
    if (av.join(',') !== bv.join(',')) {
      out.push({ name, mobil: av, cekirdek: bv });
    }
  }
  return out;
}

function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const mobilYol = arg('--mobil', 'Cargo.lock');
  const cekirdekYol = arg('--cekirdek', 'mailhere/Cargo.lock');
  const mobil = parseLock(readFileSync(mobilYol, 'utf8'));
  const cekirdek = parseLock(readFileSync(cekirdekYol, 'utf8'));
  const farklar = compare(mobil, cekirdek);
  if (farklar.length === 0) {
    console.log(`Kilitler uyumlu: ${sharedNames(mobil, cekirdek).length} ortak paket aynı sürümde.`);
    process.exit(0);
  }
  console.error('Ortak paket sürümleri iki kilitte farklı:');
  for (const f of farklar) {
    console.error(`  ${f.name}: mobil ${f.mobil.join(', ')} · çekirdek ${f.cekirdek.join(', ')}`);
  }
  console.error('Düzeltme: `cargo update -p <paket> --precise <sürüm>` ile mailhere/Cargo.lock sürümüne eşle.');
  process.exit(1);
}
