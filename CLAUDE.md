# Mailhere Android — program kuralları

Bu depo, Mailhere e-posta istemcisinin Android kabuğudur. Ortak çekirdek ve arayüz `mailhere/` git alt modülünden gelir; **mailhere'nin [`CLAUDE.md`](mailhere/CLAUDE.md) kuralları (temiz oda, tasarım dili, dil, güvenlik, "Mobil kuralları") burada da aynen bağlayıcıdır.** Bu dosya yalnız bu depoya özgü kuralları ekler.

## Nerede ne geliştirilir

- **Ortak kod alt modülde değiştirilmez.** `crates/*`, `crates/mh-app`, `app/` ve belgeler mailhere deposunda geliştirilir, oraya commit edilir; burada alt modül yeni commit'e alınır. Alt modül içinde yerel değişiklikle derlenmiş bir kabuk commit edilmez.
- Bu depoda yaşayan şeyler: `src-tauri/` (Android kabuğu: `Platform` uygulaması, anahtar kasası köprüsü, bildirim, dosya işleri, yalnız Android komutları), `src-tauri/gen/android/` (Gradle projesi, Kotlin eklentisi, manifest, kaynaklar), `scripts/`, `.github/`, `docs/ANDROID.md`.
- Yeni **ortak** komut mailhere'de üç yere girer (`shared_commands!`, `commands.txt`, masaüstü `capabilities`); burada yalnız `src-tauri/capabilities/main.json`a `allow-*` izni eklenir. Yeni **Android** komutu `src-tauri/commands.txt`e, `lib.rs`teki `shared_commands!` çağrısına ve `capabilities/main.json`a girer; `lib.rs`teki birim testi üçünü eşler. Masaüstüne özgü komutlara (`open_quick`, `notify_*`, `demo_notify`, `update_*`) burada izin verilmez.
- Kabuk kodunda `cfg(target_os = "android")` serbesttir (masaüstünde geliştirme ikilisi için dallanma); `mh-app`te değildir.

## Alt modül güncelleme akışı

```bash
git -C mailhere fetch origin
git -C mailhere checkout <commit ya da main>
cargo update --workspace                 # kilit mailhere'nin bağımlılıklarına uyar
node scripts/kilit-denetle.mjs           # ortak paket sürümleri iki kilitte aynı mı
pnpm install                             # arayüz bağımlılıkları değiştiyse pnpm-lock.yaml yenilenir
cargo test -p mailhere-android && cargo clippy --locked --target aarch64-linux-android -p mailhere-android --all-targets -- -D warnings
git add mailhere Cargo.lock pnpm-lock.yaml && git commit -m "Alt modül: mailhere <kısa özet>"
```

- Alt modül **daima** mailhere `main`inde bulunan bir commit'e sabitlenir.
- Sürüm üçlüsü (`package.json`, `src-tauri/tauri.conf.json`, `Cargo.toml`) alt modülün sürümüyle aynı tutulur; `node scripts/surum-denetle.mjs` denetler. `versionCode = X*1_000_000 + Y*10_000 + Z*100 + R` (`tauri.conf.json` → `bundle.android.versionCode`).
- `src-tauri/gen/android` commit edilir; her derlemede üretilen dosyalar (`tauri.properties`, `app/tauri.build.gradle.kts`, `tauri.settings.gradle`, `generated/`, `jniLibs/`, `build/`) `.gitignore`dadır. `tauri android init` yeniden çalıştırılmaz; şablon güncellemesi gerekiyorsa elle ve gözden geçirilerek alınır.

## Komutlar

```bash
pnpm install                                   # arayüz bağımlılıkları (alt modülün app paketi)
pnpm tauri android dev                         # cihaz ya da emülatörde
pnpm tauri android build --apk --target aarch64 [--debug]
cargo run -p mailhere-android                  # kabuğu masaüstünde aç (bellek kipi, yalnız geliştirme)
cargo test -p mailhere-android                 # host testleri (yetenek ↔ komut eşitliği, kasa köprüsü)
cargo clippy --locked --target aarch64-linux-android -p mailhere-android --all-targets -- -D warnings
node scripts/kilit-denetle.mjs && node scripts/surum-denetle.mjs && node --test "scripts/*.test.mjs"
scripts/apk-denetle.sh <apk> [--debug]         # paket adı, izinler, minSdk, ABI, 16 KB hizası
```

Ortam değişkenleri ve kurulum: `docs/ANDROID.md`.

## Git

- Dal: `main`. Commit mesajı Türkçe, ne değiştiğini söyler.
- Yazar yalnız **Furkan YILDIRIM <posta@furkanyildirim.com>**. Commit ve PR metnine `Co-Authored-By`, `Claude-Session`, "Generated with" ya da benzeri imza **eklenmez** (yerel `commit-msg` kancası reddeder).
- Push'tan önce: `cargo test -p mailhere-android`, Android ve host clippy, `node scripts/kilit-denetle.mjs`, `node scripts/surum-denetle.mjs`, `pnpm --filter app check` temiz; debug APK üretilip `scripts/apk-denetle.sh` geçmeli.
- İmza anahtarı, `keystore.properties`, `*.jks` asla depoya girmez.
