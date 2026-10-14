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
    src/qr.rs               qr_scan: QrScanActivity'yi açar, okunan aktarım kodunu döner
    commands.txt            yalnız Android komutları (ortak liste alt modülde)
    capabilities/main.json  platforms ["android"], tek pencere; masaüstü komutlarına izin yok
    tauri.conf.json         frontendDist ../mailhere/app/dist, identifier, minSdk 24, versionCode
    gen/android/            Gradle projesi (commit edilir)
      app/src/main/java/com/furkanyildirim/mailhere/MainActivity.kt
      app/src/main/java/com/furkanyildirim/mailhere/plugin/{MhPlugin,SecretVault,QrScanActivity}.kt
      app/src/main/AndroidManifest.xml · res/xml/{ekler_yollari,veri_cikarma_kurallari}.xml
      res/layout/qr_tara.xml · res/drawable/{ic_geri,ic_fener,qr_uyari_zemin}.xml · res/values{,-en}/strings.xml
```

- **Komutlar:** `mh_app::shared_commands![files::attachment_share, files::attachment_save, notify::notification_permission, qr::qr_scan]`. Ortak komutların gövdesi alt modülde (QR aktarımın `transfer_begin/cancel/receive/apply` komutları dahil; telefonda şimdilik yalnız `receive` ve `apply` kullanılır, `begin/cancel` ortak listede olduğundan izinlidir); kabuğa özgü iş `Platform` üzerinden gelir.
- **Pencere:** tek `main` penceresi `index.html#/main`; yönlendirme bekçisi (`mh_app::routes::is_app_url`) uygulama dışı adresi engeller, geliştirme sunucusuna yalnız debug derlemesinde izin verir. Hızlı okuma `mh:open`, yazma `mh:navigate { route }` (`mh_core::events::NAVIGATE`, `NavigateEvent`) olayıyla aynı pencerede açılır.
- **Veri yolları:** veritabanı `<dataDir>/files/mailhere.db` (`app_local_data_dir` + `files`), önbellek `cacheDir`. Açılan ekler `cache/ekler/<posta>/<ad>`, seçilen ekler `cache/giden/<uuid>/<ad>`.
- **Kasa:** `SecretVault.kt` — `AndroidKeyStore` AES-256-GCM (StrongBox varsa, yoksa TEE; kimlik doğrulama şartı yok), Keystore'un ürettiği 12 B IV, AAD = anahtar adı, `IV||şifre||etiket` Base64 ile `mailhere-kasa.xml` (MODE_PRIVATE). Veritabanı anahtarı `db-key` adıyla aynı kasada. Rust tarafı (`keystore.rs`) yalnız JSON eşler; hata metinleri değer taşımaz.
- **Dosyalar:** seçici `tauri-plugin-dialog` (SAF), içerik `tauri-plugin-fs` ile `content://`dan okunur; açma `ACTION_VIEW`, paylaşma `ACTION_SEND`, ikisi de `FileProvider` (`<paket>.ekler`, yalnız `cache/ekler/`) ve dosya başına geçici okuma izniyle. Güvenli olmayan türler (`opens_directly` dışı) doğrudan açılmaz, paylaşım sayfasına verilir.
- **Bildirim:** kanal `yeni-posta` (HIGH); başlık "gönderen · hesap", gövde konu; `extra.id` ile dokununca `mh:open`. Android 13+ izni `notification_permission` komutuyla istenir (`granted` | `denied` | `prompt` | `promptWithRationale`; arayüz aktarım bittikten sonra çağırır). Kitaplıktan gelen `RECEIVE_BOOT_COMPLETED` / `WAKE_LOCK` izinleri ve açılış alıcısı manifestte atılır.
- **QR tarayıcı:** `qr_scan` → Kotlin `scanQr` → tam ekran `QrScanActivity` (`exported=false`, ekran yönü serbest, koyu tema). CameraX (`camera-core/camera2/lifecycle/view` 1.5.x) arka kamera önizlemesi + `ImageAnalysis` (~720p, yalnız en son kare); her karenin Y düzlemi parlaklık dizisine alınır, önce çerçeve kılavuzundaki orta kare sensör dönüşüne göre dik çevrilip, sonra tüm kare, düz ve ters renkle `zxing core` 3.5.x ile çözülür. İkisi Apache-2.0; ML Kit ya da Google Play Hizmetleri yoktur (`apk-denetle.sh` dex'te `gms`/`mlkit` arar). Yalnız `mailhere://aktar?` ile başlayan metin kabul edilir; başka QR'da kısa uyarı gösterilir, tarama sürer; bulunca titreşimsiz kapanır. Sözleşme: `qr_scan() → string | null` (vazgeçince `null`; masaüstü geliştirme ikilisinde her zaman `null`, arayüz kodu yapıştırmaya düşer). Kamera izni çalışma anında, yalnız bu ekranda istenir; reddedilirse "Kamera izni verilmedi; Ayarlar'dan izin ver ya da kodu yapıştır." hatası, kamera açılamazsa "Kamera açılamadı…" hatası döner. Ekran metinleri `res/values/strings.xml` (Türkçe) ve `values-en` (İngilizce); renkler koyu temanın jetonları (`#0F1115` zemin, `#22252D` kart, `#E6E8ED` yazı, `#8C96F6` vurgu); yazı tipi sistem yazı tipidir (Geist yalnız web arayüzünde, alt modülde ttf yok). Üst çubuk 56 dp, düğmeler 48 dp, yazı 15 sp.
- **Günlük:** `tracing-android` → logcat etiketi `Mailhere`; protokol kitaplıklarının ayrıntılı günlüğü ortak süzgeçle kapalıdır. `adb logcat -s Mailhere`.
- **Manifest:** izinler yalnız `INTERNET`, `POST_NOTIFICATIONS`, `CAMERA` (+ `uses-feature android.hardware.camera required=false`: kamerasız cihazda kurulur, kod yapıştırılır; bir de androidx.core'un her uygulamada tanımladığı, uygulamaya özel imza düzeyi `<paket>.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION`; sistem izni değildir); `allowBackup=false`, `fullBackupContent=false`, `dataExtractionRules` tümünü dışlar; açılış zemini `#ECEEF2` / gece `#0F1115`. Geliştirme paketi (`--debug`, `pnpm tauri android dev`) yalnız Vite sunucusu için `usesCleartextTraffic=true` taşır (`src/debug/AndroidManifest.xml`); sürüm paketinde yoktur.

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

`CI` iş akışı (push main + PR): alt modül recursive; pnpm + Node 22; kilit ve sürüm denetimi; `pnpm --filter app check/test`; Rust stable + `aarch64-linux-android`; Java 21; NDK 28.2 (yoksa sdkmanager); `cargo fmt --check`; Android clippy (`-D warnings`); bağımlılık ağacında `keyring`, `tauri-plugin-updater`, `tauri-plugin-single-instance` yok; host clippy + `cargo test`; debug APK; `scripts/apk-denetle.sh` (paket adı, izinler INTERNET/POST_NOTIFICATIONS/CAMERA, kamera özelliği isteğe bağlı, minSdk 24, arm64-v8a, manifest, QrScanActivity dışa kapalı, dex'te Play Hizmetleri / ML Kit yok ve zxing var, 16 KB hizası); APK 14 gün artifact.

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
15. **QR aktarım — izin:** "Bilgisayardan al" ilk kez açılınca kamera izni sorulur; reddedilince "Kamera izni verilmedi; Ayarlar'dan izin ver ya da kodu yapıştır." görünür ve kod yapıştırma yolu çalışır; "Bir daha sorma" sonrasında sistem iletişim kutusu çıkmadan aynı hata gelir. Kamerasız cihaz/emülatörde "Kamera açılamadı…" hatası ve yapıştırma yolu.
16. **QR aktarım — tarayıcı:** koyu tam ekran, önizleme akıcı, çerçeve kılavuzu ortada, sistem çubukları saydam (çentik/geçişli ekranda üst çubuk güvenli alanda); geri düğmesi ve sistem geri tuşu `null` ile kapatır; fener düğmesi yalnız flaşlı cihazda görünür, açınca vurgu rengine döner, ekran kapanınca söner; döndürünce ekran yeniden oluşturulmaz ve okuma sürer; başka bir QR'da (ör. web adresi) "Bu bir Mailhere aktarım kodu değil" 1,8 s görünür, tarama devam eder; masaüstündeki QR ters (koyu tema) gösterilirken de okunur.
17. **QR aktarım — uçtan uca:** bilgisayarda Ayarlar › Cihazlar › "Telefona aktar" → telefonda QR okut → özet (hesaplar, kurallar, etiketler, ayarlar) → uygula → bilgisayarda "uygulandı" durumu → telefonda bildirim izni isteği (13+) → gelen kutusu eşitlenir; parolasız hesap raporda listelenir. Aynı Wi-Fi'de değilken (hücresel veri) 30 s içinde anlaşılır ağ hatası; kodu yapıştırarak alma aynı sonucu verir; 3 dk sonra süresi dolan kod reddedilir.

## 7. Bilinen sınırlar (Aşama 4d)

- Arayüzün `qr_scan`, `notification_permission`, `attachment_share`, `attachment_save` komutlarını sözleşmeye (`app/src/lib/api/types.ts`) ve sahte arka uca eklemesi ile "Bilgisayardan al" ekranının tarayıcıyı çağırması alt modülün Aşama 4c işidir; kabuk tarafı hazırdır.
- Telefondan bilgisayara aktarım (telefonda `transfer_begin`) henüz arayüzde yok; komut ortak listede olduğundan izinlidir.
- Arka plan eşitlemesi (WorkManager) Aşama 7.
