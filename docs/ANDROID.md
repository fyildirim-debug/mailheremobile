# Mailhere Android — geliştirme ve sürüm

Bu belge Android kabuğunun kurulumunu, mimarisini, sürüm şemasını ve cihazda denetlenecekleri anlatır. Tasarım ve güvenlik kuralları için alt modüldeki `docs/TASARIM.md` (§12 Mobil uyarlama) ve `docs/GUVENLIK.md` ("Android") geçerlidir.

## 1. Kurulum

Gerekenler:

| Araç | Sürüm | Not |
| --- | --- | --- |
| Rust (stable) | ≥ 1.90 | `rustup target add aarch64-linux-android` |
| Android SDK | platforms `android-36`, build-tools `36.0.0`, cmdline-tools | `sdkmanager --install "platforms;android-36" "build-tools;36.0.0"` |
| Android NDK | **28.2.13676358** | `sdkmanager --install "ndk;28.2.13676358"`; CI aynı sürümü kullanır |
| Java | 21 (Temurin) | Gradle 9 ve AGP 9 için |
| Node | 22 · pnpm 10 | `corepack enable` |

Ortam değişkenleri (kabuk profiline):

```bash
export ANDROID_HOME=/opt/android-sdk                        # kendi yolun
export NDK_HOME=$ANDROID_HOME/ndk/28.2.13676358
export ANDROID_NDK_HOME=$NDK_HOME
# SQLCipher'in OpenSSL'i kaynaktan derlenir (openssl-src); cc ve openssl-src NDK'nın clang'ını ister.
BIN=$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin
export CC_aarch64_linux_android=$BIN/aarch64-linux-android24-clang
export CXX_aarch64_linux_android=$BIN/aarch64-linux-android24-clang++
export AR_aarch64_linux_android=$BIN/llvm-ar
export RANLIB_aarch64_linux_android=$BIN/llvm-ranlib
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$BIN/aarch64-linux-android24-clang
```

Depo:

```bash
git clone --recurse-submodules https://github.com/fyildirim-debug/mailheremobile
cd mailheremobile
pnpm install                       # alt modülün app paketi (pnpm-workspace.yaml)
```

## 2. Komutlar

```bash
pnpm tauri android dev                                  # cihaz/emülatörde; arayüz Vite'tan (http://<ip>:5173)
pnpm tauri android build --apk --target aarch64 --debug # debug APK
pnpm tauri android build --apk --aab --target aarch64   # sürüm (keystore.properties varsa imzalı)
cargo run -p mailhere-android                           # kabuğu masaüstünde aç (bellek kipi; yalnız debug derlemesi)
cargo test -p mailhere-android                          # host testleri
cargo clippy --locked --target aarch64-linux-android -p mailhere-android --all-targets -- -D warnings
node scripts/kilit-denetle.mjs                          # iki Cargo.lock'ta ortak paket sürümleri aynı mı
node scripts/surum-denetle.mjs [--etiket vX.Y.Z]        # sürüm üçlüsü, alt modül sürümü, versionCode
scripts/apk-denetle.sh <apk> [--debug]                  # badging, izinler, manifest, 16 KB hizası
```

Çıktılar: `src-tauri/gen/android/app/build/outputs/apk/universal/{debug,release}/app-universal-{debug,release}.apk` (yalnız istenen ABI'yi içerir), `.../bundle/universalRelease/*.aab`. Debug APK ayıklama sembolleri taşıdığından büyüktür (~100 MB); sürüm paketi `strip` ve LTO ile küçülür.

`pnpm tauri android dev` için telefon ve bilgisayar aynı ağda olmalı; geliştirme paketinde (`src/debug/AndroidManifest.xml`) açık metin trafiği yalnız bunun için açıktır, sürüm paketinde yoktur.

## 3. Mimari

```
mailheremobile/
  mailhere/                 git alt modülü (sabit commit): crates/*, crates/mh-app, app/
  src-tauri/
    src/lib.rs              run(): günlük (logcat), tokio, eklentiler, setup_core, tek pencere
    src/main.rs             masaüstünde geliştirme ikilisi (MAILHERE_SECRETS=memory)
    src/platform.rs         AndroidPlatform: Platform trait'i (bildirim, tek pencere, dosya, onay, açılış hatası)
    src/keystore.rs         AndroidKeystoreSecrets: SecretStore → Kotlin SecretVault (secretGet/Set/Delete)
    src/plugin.rs           "mh-android" uygulama içi eklenti; MhPlugin.kt kaydı, run_mobile_plugin köprüsü
    src/files.rs            seçme (content:// → cache/giden/<uuid>/<ad>), kaydet (SAF), aç/paylaş; attachment_share/save
    src/notify.rs           kanal yeni-posta, bildirim, dokununca açma, notification_permission
    commands.txt            yalnız Android komutları (ortak liste alt modülde)
    capabilities/main.json  platforms ["android"], tek pencere; masaüstü komutlarına izin yok
    tauri.conf.json         frontendDist ../mailhere/app/dist, identifier, minSdk 24, versionCode
    gen/android/            Gradle projesi (commit edilir)
      app/src/main/java/com/furkanyildirim/mailhere/MainActivity.kt
      app/src/main/java/com/furkanyildirim/mailhere/plugin/{MhPlugin,SecretVault}.kt
      app/src/main/AndroidManifest.xml · res/xml/{ekler_yollari,veri_cikarma_kurallari}.xml
```

- **Komutlar:** `mh_app::shared_commands![files::attachment_share, files::attachment_save, notify::notification_permission]`. Ortak komutların gövdesi alt modülde; kabuğa özgü iş `Platform` üzerinden gelir.
- **Pencere:** tek `main` penceresi `index.html#/main`; yönlendirme bekçisi (`mh_app::routes::is_app_url`) uygulama dışı adresi engeller, geliştirme sunucusuna yalnız debug derlemesinde izin verir. Hızlı okuma `mh:open`, yazma `mh:navigate { route }` olayıyla aynı pencerede açılır.
- **Veri yolları:** veritabanı `<dataDir>/files/mailhere.db` (`app_local_data_dir` + `files`), önbellek `cacheDir`. Açılan ekler `cache/ekler/<posta>/<ad>`, seçilen ekler `cache/giden/<uuid>/<ad>`.
- **Kasa:** `SecretVault.kt` — `AndroidKeyStore` AES-256-GCM (StrongBox varsa, yoksa TEE; kimlik doğrulama şartı yok), Keystore'un ürettiği 12 B IV, AAD = anahtar adı, `IV||şifre||etiket` Base64 ile `mailhere-kasa.xml` (MODE_PRIVATE). Veritabanı anahtarı `db-key` adıyla aynı kasada. Rust tarafı (`keystore.rs`) yalnız JSON eşler; hata metinleri değer taşımaz.
- **Dosyalar:** seçici `tauri-plugin-dialog` (SAF), içerik `tauri-plugin-fs` ile `content://`dan okunur; açma `ACTION_VIEW`, paylaşma `ACTION_SEND`, ikisi de `FileProvider` (`<paket>.ekler`, yalnız `cache/ekler/`) ve dosya başına geçici okuma izniyle. Güvenli olmayan türler (`opens_directly` dışı) doğrudan açılmaz, paylaşım sayfasına verilir.
- **Bildirim:** kanal `yeni-posta` (HIGH); başlık "gönderen · hesap", gövde konu; `extra.id` ile dokununca `mh:open`. Android 13+ izni `notification_permission` komutuyla istenir. Kitaplıktan gelen `RECEIVE_BOOT_COMPLETED` / `WAKE_LOCK` izinleri ve açılış alıcısı manifestte atılır.
- **Günlük:** `tracing-android` → logcat etiketi `Mailhere`; protokol kitaplıklarının ayrıntılı günlüğü ortak süzgeçle kapalıdır. `adb logcat -s Mailhere`.
- **Manifest:** izinler yalnız `INTERNET`, `POST_NOTIFICATIONS` (bir de androidx.core'un her uygulamada tanımladığı, uygulamaya özel imza düzeyi `<paket>.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION`; sistem izni değildir); `allowBackup=false`, `fullBackupContent=false`, `dataExtractionRules` tümünü dışlar; açılış zemini `#ECEEF2` / gece `#0F1115`. Geliştirme paketi (`--debug`, `pnpm tauri android dev`) yalnız Vite sunucusu için `usesCleartextTraffic=true` taşır (`src/debug/AndroidManifest.xml`); sürüm paketinde yoktur.

## 4. Sürüm şeması

- Sürüm `X.Y.Z` üç yerde aynıdır: `package.json`, `src-tauri/tauri.conf.json`, `Cargo.toml` (`workspace.package`); alt modülün sürümüyle de aynı tutulur.
- `versionCode = X*1_000_000 + Y*10_000 + Z*100 + R`; `R` (0–99) yalnız aynı sürümün Android'e özgü yeniden paketlenmesi için artar. `0.3.1` → `30100`. `tauri.conf.json` → `bundle.android.versionCode`.
- Etiket `vX.Y.Z`; `node scripts/surum-denetle.mjs --etiket vX.Y.Z` tutarlılığı denetler.
- Sürüm notları `docs/surumler/<etiket>.md`; yoksa etiket mesajı ya da commit mesajı kullanılır.

Sürüm çıkarma: `Sürüm` iş akışı (etiket gönderimi ya da elle `surum` girdisi). Gizli değerler: `ANDROID_KEYSTORE_BASE64` (`base64 -w0 yayin.jks`), `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`; isteğe bağlı `MAILHERE_GOOGLE_CLIENT_ID`, `MAILHERE_GOOGLE_CLIENT_SECRET`, `MAILHERE_MICROSOFT_CLIENT_ID`. İş akışı `keystore.properties` yazar, APK + AAB üretir, `apksigner verify` / `jarsigner -verify` ile doğrular, `SHA256SUMS` ekler ve GitHub sürümü yayımlar. Anahtar yoksa paket imzasız üretilir ve uyarı verilir.

Yerelde imzalı paket: `src-tauri/gen/android/keystore.properties` (gitignore'da):

```properties
storeFile=/mutlak/yol/yayin.jks
storePassword=…
keyAlias=…
keyPassword=…
```

## 5. CI

`CI` iş akışı (push main + PR): alt modül recursive; pnpm + Node 22; kilit ve sürüm denetimi; `pnpm --filter app check/test`; Rust stable + `aarch64-linux-android`; Java 21; NDK 28.2 (yoksa sdkmanager); `cargo fmt --check`; Android clippy (`-D warnings`); bağımlılık ağacında `keyring`, `tauri-plugin-updater`, `tauri-plugin-single-instance` yok; host clippy + `cargo test`; debug APK; `scripts/apk-denetle.sh` (paket adı, izinler, minSdk 24, arm64-v8a, manifest, 16 KB hizası); APK 14 gün artifact.

## 6. Cihaz denetim listesi

Bu ortamda cihaz/emülatör çalıştırılamadığından aşağıdakiler gerçek cihazda doğrulanır (her sürümde, en az Android 7 ve Android 14+ birer cihaz):

1. **Açılış:** ilk açılışta beyaz parlama yok (açık/koyu tema zemin rengi); arayüz yüklenir; `adb logcat -s Mailhere` "veri klasörü" satırını gösterir.
2. **Kasa:** hesap ekle → uygulamayı kapat/aç → parola sorulmadan eşitlenir (`mailhere-kasa.xml` var, düz metin içermez). Hesabı kaldır → kayıt silinir. StrongBox'lı ve StrongBox'sız cihazda anahtar üretimi.
3. **Veritabanı:** `files/mailhere.db` SQLCipher ile şifreli (`strings` ile okunamaz); uygulama verisi `adb backup` ile çıkmaz (`allowBackup=false`).
4. **Bildirim:** Android 13+'ta izin isteği (`notification_permission`); yeni postada kanal `yeni-posta` bildirimi; dokununca ilgili posta açılır (`mh:open`); sessize alma ve hesap başına bildirim kapatma çalışır.
5. **Ekler:** eki aç (PDF/resim uygun uygulamada, FileProvider adresi `content://com.furkanyildirim.mailhere.ekler/ekler/...`); güvenli olmayan tür paylaşım sayfasına düşer; "Paylaş" (`attachment_share`) ve "Kaydet" (`attachment_save`, SAF) çalışır; ek eklerken seçilen dosya önbelleğe kopyalanır ve gönderilen postada adı/içeriği doğru gider.
6. **Yazma ve okuma:** `open_compose` → `mh:navigate` ile yazma yığını açılır; `.eml` kaydetme SAF ile; takvim daveti `.ics` takvim uygulamasında açılır.
7. **Sertifika onayı:** kendinden imzalı sunucuda onay iletişim kutusu (ebeveynsiz `confirm`) görünür, ret/onay doğru işler.
8. **Açılış hatası:** kasa erişilemez durumdayken (örn. test için kasa kaydı bozulmuş) hata iletişim kutusu gösterilir, "Kapat" uygulamayı kapatır; dosya silinmez.
9. **OAuth:** Google/Microsoft girişi tarayıcıda açılır, `127.0.0.1` dönüşü uygulamaya gelir (cihazda doğrulanmalı).
10. **Ağ:** uçak kipi → kapat → eşitleme kendiliğinden toparlar; IDLE/JMAP anlık bildirim arka planda kesilince yeniden kurulur (sınırlı; Aşama 7 arka plan eşitlemesi).
11. **Geri tuşu:** mobil arayüz geldiğinde çekmece/alt sayfa/yığın sırası; kökte uygulama kapanır.
12. **Performans:** 10 bin postalı hesapta liste kaydırması akıcı; soğuk açılış < 2 s (orta sınıf cihaz).
13. **16 KB sayfa:** Android 15+ 16 KB cihaz/emülatörde kurulup açılır (`scripts/apk-denetle.sh` hizayı doğrular).
14. **Güncelleme:** önceki sürümün üstüne kurulum veriyi korur; `versionCode` artmış olmalı.

## 7. Bilinen sınırlar (Aşama 2)

- Mobil arayüz henüz alt modülde değil; uygulama masaüstü arayüzünü dar ekranda açar.
- QR ile aktarım (`qr_scan`, `transfer_*`) ve kamera izni Aşama 4d'de eklenir.
- Arka plan eşitlemesi (WorkManager) Aşama 7.
