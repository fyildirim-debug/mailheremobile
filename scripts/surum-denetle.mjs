#!/usr/bin/env node
// Sürüm tutarlılığı: package.json, src-tauri/tauri.conf.json ve Cargo.toml
// (workspace.package) aynı sürümü taşımalı; alt modülün (mailhere) sürümüyle
// aynı olmalı; Android versionCode şemaya uymalı:
//
//   versionCode = X*1_000_000 + Y*10_000 + Z*100 + R   (R: yalnız Android yeniden paketleme, 0–99)
//
//   node scripts/surum-denetle.mjs [--etiket v0.3.1] [--kok .]
// Uyuşmazlıkta çıkış kodu 1.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** "0.3.1" → { major, minor, patch }. */
export function parseVersion(v) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(String(v ?? '').trim());
  if (!m) throw new Error(`Sürüm X.Y.Z biçiminde değil: ${v}`);
  return { major: Number(m[1]), minor: Number(m[2]), patch: Number(m[3]) };
}

/** Şemaya göre versionCode (R yeniden paketleme sayacı). */
export function versionCodeOf(version, revision = 0) {
  const { major, minor, patch } = parseVersion(version);
  if (minor > 99 || patch > 99 || revision > 99 || revision < 0) {
    throw new Error('Alt sürüm, yama ve yeniden paketleme sayacı 0–99 arasında olmalı');
  }
  return major * 1_000_000 + minor * 10_000 + patch * 100 + revision;
}

/** versionCode verilen sürüme ait mi (R serbest)? */
export function versionCodeMatches(version, code) {
  return Number.isInteger(code) && code - versionCodeOf(version, 0) >= 0 && code - versionCodeOf(version, 0) < 100;
}

/** Cargo.toml içindeki `[workspace.package]` sürümü. */
export function cargoWorkspaceVersion(text) {
  const section = /\[workspace\.package\]([\s\S]*?)(?:\n\[|$)/.exec(text);
  const m = section && /^\s*version\s*=\s*"([^"]+)"/m.exec(section[1]);
  if (!m) throw new Error('Cargo.toml [workspace.package] version bulunamadı');
  return m[1];
}

/** Tüm denetimler; hata listesi (boşsa tutarlı). */
export function check({ pkg, conf, cargo, corePkg, etiket }) {
  const errors = [];
  const v = pkg.version;
  if (conf.version !== v) errors.push(`tauri.conf.json sürümü (${conf.version}) package.json ile (${v}) aynı değil`);
  const cv = cargoWorkspaceVersion(cargo);
  if (cv !== v) errors.push(`Cargo.toml sürümü (${cv}) package.json ile (${v}) aynı değil`);
  if (corePkg && corePkg.version !== v) {
    errors.push(`mailhere alt modülünün sürümü (${corePkg.version}) bu deponun sürümüyle (${v}) aynı değil`);
  }
  const code = conf.bundle?.android?.versionCode;
  if (!versionCodeMatches(v, code)) {
    errors.push(`versionCode (${code}) sürüme (${v}) uymuyor; beklenen ${versionCodeOf(v, 0)}–${versionCodeOf(v, 99)}`);
  }
  if (etiket !== undefined) {
    if (!/^v\d+\.\d+\.\d+$/.test(etiket)) errors.push(`Geçersiz sürüm etiketi: ${etiket} (ör. v0.3.1)`);
    else if (etiket !== `v${v}`) errors.push(`Etiket (${etiket}) paket sürümüyle (v${v}) aynı değil`);
  }
  return errors;
}

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const kok = arg('--kok') ?? '.';
  const read = (p) => readFileSync(join(kok, p), 'utf8');
  const errors = check({
    pkg: JSON.parse(read('package.json')),
    conf: JSON.parse(read('src-tauri/tauri.conf.json')),
    cargo: read('Cargo.toml'),
    corePkg: JSON.parse(read('mailhere/package.json')),
    etiket: arg('--etiket'),
  });
  if (errors.length === 0) {
    const pkg = JSON.parse(read('package.json'));
    console.log(`Sürüm tutarlı: ${pkg.version}`);
    process.exit(0);
  }
  for (const e of errors) console.error(`Sürüm hatası: ${e}`);
  process.exit(1);
}
