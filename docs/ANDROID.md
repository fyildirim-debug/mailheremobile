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
    src/notify.rs           yeni posta bildirimi (yığın → hesap başına grup), okunanı kaldırma, notification_permission/status/settings
    src/eylem.rs            bildirim eylemleri → çekirdek (Okundu, Arşivle, Yanıtla, Geri al, aç, uyan); yanıt postası
    src/arka.rs             battery_status/settings, background_mode_get/set (bildirim sağlığı)
    src/haptic.rs           haptic: dokunsal geri bildirim
    src/bars.rs             system_bars: durum/gezinme çubuğu simgeleri
    src/qr.rs               qr_scan: QrScanActivity'yi açar, okunan aktarım kodunu döner
    commands.txt            yalnız Android komutları (ortak liste alt modülde)
    capabilities/main.json  platforms ["android"], tek pencere; masaüstü komutlarına izin yok
    tauri.conf.json         frontendDist ../mailhere/app/dist, identifier, minSdk 24, versionCode
    gen/android/            Gradle projesi (commit edilir)
      app/src/main/java/com/furkanyildirim/mailhere/MainActivity.kt
      app/src/main/java/com/furkanyildirim/mailhere/plugin/{MhPlugin,SecretVault,QrScanActivity}.kt
      app/src/main/java/com/furkanyildirim/mailhere/plugin/{Bildirimler,BildirimAlici,ArkaPlanHizmeti}.kt
      app/src/main/AndroidManifest.xml · res/xml/{ekler_yollari,veri_cikarma_kurallari}.xml
      res/layout/qr_tara.xml · res/drawable/{ic_geri,ic_fener,qr_uyari_zemin,ic_bildirim,ic_okundu,ic_arsiv,ic_yanit,ic_geri_al,ic_kapat}.xml
      res/values{,-en}/strings.xml · res/values{,-night}/colors.xml
```

- **Komutlar:** `mh_app::shared_commands![files::attachment_share, files::attachment_save, notify::notification_permission, notify::notification_settings, notify::notification_status, qr::qr_scan, bars::system_bars, arka::battery_status, arka::battery_settings, arka::background_mode_get, arka::background_mode_set, haptic::haptic]` (liste `commands.txt`, izinler `capabilities/main.json`; `lib.rs`teki test üçünü eşler). Ortak komutların gövdesi alt modülde (QR aktarımın `transfer_begin/cancel/receive/apply` komutları dahil; telefonda şimdilik yalnız `receive` ve `apply` kullanılır, `begin/cancel` ortak listede olduğundan izinlidir); kabuğa özgü iş `Platform` üzerinden gelir.
- **Pencere:** tek `main` penceresi `index.html#/main`; yönlendirme bekçisi (`mh_app::routes::is_app_url`) uygulama dışı adresi engeller, geliştirme sunucusuna yalnız debug derlemesinde izin verir. Hızlı okuma `mh:open`, yazma `mh:navigate { route }` (`mh_core::events::NAVIGATE`, `NavigateEvent`) olayıyla aynı pencerede açılır.
- **Veri yolları:** veritabanı `<dataDir>/files/mailhere.db` (`app_local_data_dir` + `files`), önbellek `cacheDir`. Açılan ekler `cache/ekler/<posta>/<ad>`, seçilen ekler `cache/giden/<uuid>/<ad>`.
- **Kasa:** `SecretVault.kt` — `AndroidKeyStore` AES-256-GCM (StrongBox varsa, yoksa TEE; kimlik doğrulama şartı yok), Keystore'un ürettiği 12 B IV, AAD = anahtar adı, `IV||şifre||etiket` Base64 ile `mailhere-kasa.xml` (MODE_PRIVATE). Veritabanı anahtarı `db-key` adıyla aynı kasada. Rust tarafı (`keystore.rs`) yalnız JSON eşler; hata metinleri değer taşımaz.
- **Dosyalar:** seçici `tauri-plugin-dialog` (SAF), içerik `tauri-plugin-fs` ile `content://`dan okunur; açma `ACTION_VIEW`, paylaşma `ACTION_SEND`, ikisi de `FileProvider` (`<paket>.ekler`, yalnız `cache/ekler/`) ve dosya başına geçici okuma izniyle. Güvenli olmayan türler (`opens_directly` dışı) doğrudan açılmaz, paylaşım sayfasına verilir.
- **Bildirim:** `Platform::notify` → `notify.rs` aynı eşitlemede gelen postaları 350 ms toplar → `MhPlugin.postaBildir` → `Bildirimler.kt` (`NotificationCompat`; eklentinin kurucusu grup özeti, açık sürüm ve etkinliksiz eylem desteklemediği için kullanılmaz, yalnız izin komutları eklentiden). Kanal `yeni-posta` (HIGH, adı/açıklaması `strings.xml`); posta başına bildirim (etiket `posta:<id>`): başlık gönderen, alt metin hesap adı, gövde konu (genişletince kalın konu + önizlemenin ilk satırı; gövde ve ek adı yok), kategori `email`. Hesap başına grup `mailhere:<hesap>` ve sabit etiketli özet `ozet:<hesap>` (InboxStyle: "N yeni posta", en çok 6 satır gönderen + konu). Bir yığında yalnız en yeni posta ses çıkarır ve üstten görünür; öteki postalar ve özet sessizdir (`GROUP_ALERT_CHILDREN` + `setSilent`: çocuk `GROUP_ALERT_SUMMARY`, özet `GROUP_ALERT_CHILDREN` olur). Kilit ekranı `VISIBILITY_PRIVATE` + açık sürüm (yalnız hesap adı ve "Yeni posta" / "N yeni posta"); sistem "hassas içeriği gizle" seçiliyse o görünür. Dokununca `MainActivity` (`singleTask`, `POSTA_AC`) → `MhPlugin` bildirimi kaldırır, Rust'a `ac` olayı → `Platform::open_main` → `mh:open`; soğuk açılışta dokunuş Rust dinlemeye başlayınca alınır ve sayfa yüklendikten 1,2 s sonra açılır. Okunan, arşivlenen, silinen (başka cihazda okunan dahil) postanın bildirimi `counts_changed` → `reconcile_soon` (1,5 s birleştirme) ile kalkar, özet güncellenir; kullanıcının kapattığı özet yeniden kurulmaz. Elek'e düşen postalar `Platform::notify`a hiç gelmez (`mh_app::screener::screen` ayıklar). Android 13+ izni `notification_permission` komutuyla istenir (`granted` | `denied` | `prompt` | `promptWithRationale`; arayüz aktarım bittikten sonra çağırır). Kitaplıktan gelen `RECEIVE_BOOT_COMPLETED` / `WAKE_LOCK` izinleri ve açılış alıcısı manifestte atılır.
- **Bildirim eylemleri:** Okundu, Arşivle, satır içi Yanıtla (RemoteInput, sistem önerili yanıt kapalı), yanıt kuyruktayken Geri al. Düğme → `BildirimAlici` (dışa kapalı alıcı) → `MhPlugin.ilet` → Rust kanalı (`eylem.rs`, olay `bildirim`) → `mh_app::commands::messages_flag` / `messages_move(Archive)` / giden kutusu (`Outbox::enqueue`, "gönderimi geri al" süresi; yanıt arayüzün hızlı yanıtıyla aynı: Reply-To ya da gönderen, kendi adres çıkarılır, `Re:` konu, hesabın yanıt imzası, alıntı). İşler önce yerelde uygulanır, sunucuya kalıcı işlem kuyruğuyla gider (ağ yoksa bağlantı gelince). Bitince bildirim kalkar ve özet güncellenir; yanıtta bildirim geri sayımlı "Geri al" satırına döner, süre bitince kendiliğinden kalkar; hata olursa bildirimde neden ve çözüm yazar. Süreç ölmüşse alıcı çekirdeği başlatamaz (bkz. "Arka plan"): eylem `SecretVault`ta (`bildirim-bekleyen`, AES-GCM; yanıt metni taşıyabilir) en çok 50 kayıtlık kuyruğa yazılır, bildirim "Mailhere açılınca uygulanacak" durumuna geçer; uygulama açılınca `eylem::setup` kuyruğu boşaltır ve aynı yoldan uygular.
- **Bildirim sağlığı:** `battery_status` (`unrestricted` | `optimized` | `restricted`: `PowerManager.isIgnoringBatteryOptimizations`, Android 9+ `ActivityManager.isBackgroundRestricted`), `battery_settings` ("Kısıtlı"ysa uygulama ayar sayfası, değilse `ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, açılamazsa optimizasyon listesi; kullanıcı dönünce çözülür), `background_mode_get/set` (anlık bildirim; bkz. "Arka plan"). Arayüz Ayarlar › Bildirimler'de açıklamayla gösterir; masaüstünde komutlar yoktur, satırlar gizlenir.
- **Dokunsal geri bildirim:** `haptic { kind: 'tick' | 'confirm' | 'reject' }` → `View.performHapticFeedback` (API 34+ `GESTURE_THRESHOLD_ACTIVATE`, API 30+ `CONFIRM`/`REJECT`; eski sürümde `CLOCK_TICK`/`VIRTUAL_KEY`/`LONG_PRESS`); sistemin dokunma geri bildirimi ayarına uyar, `VIBRATE` izni gerekmez.
- **QR tarayıcı:** `qr_scan` → Kotlin `scanQr` → tam ekran `QrScanActivity` (`exported=false`, ekran yönü serbest, koyu tema). CameraX (`camera-core/camera2/lifecycle/view` 1.5.x) arka kamera önizlemesi + `ImageAnalysis` (~720p, yalnız en son kare); her karenin Y düzlemi parlaklık dizisine alınır, önce çerçeve kılavuzundaki orta kare sensör dönüşüne göre dik çevrilip, sonra tüm kare, düz ve ters renkle `zxing core` 3.5.x ile çözülür. İkisi Apache-2.0; ML Kit ya da Google Play Hizmetleri yoktur (`apk-denetle.sh` dex'te `gms`/`mlkit` arar). Yalnız `mailhere://aktar?` ile başlayan metin kabul edilir; başka QR'da kısa uyarı gösterilir, tarama sürer; bulunca titreşimsiz kapanır. Sözleşme: `qr_scan() → string | null` (vazgeçince `null`; masaüstü geliştirme ikilisinde her zaman `null`, arayüz kodu yapıştırmaya düşer). Kamera izni çalışma anında, yalnız bu ekranda istenir; reddedilirse "Kamera izni verilmedi; Ayarlar'dan izin ver ya da kodu yapıştır." hatası, kamera açılamazsa "Kamera açılamadı…" hatası döner. Ekran metinleri `res/values/strings.xml` (Türkçe) ve `values-en` (İngilizce); renkler koyu temanın jetonları (`#0F1115` zemin, `#22252D` kart, `#E6E8ED` yazı, `#8C96F6` vurgu); yazı tipi sistem yazı tipidir (Geist yalnız web arayüzünde, alt modülde ttf yok). Üst çubuk 56 dp, düğmeler 48 dp, yazı 15 sp.
- **Günlük:** `tracing-android` → logcat etiketi `Mailhere`; protokol kitaplıklarının ayrıntılı günlüğü ortak süzgeçle kapalıdır. `adb logcat -s Mailhere`.
- **Manifest:** izinler yalnız `INTERNET`, `POST_NOTIFICATIONS`, `CAMERA`, `FOREGROUND_SERVICE` + `FOREGROUND_SERVICE_SPECIAL_USE` (anlık bildirim hizmeti), `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` (kullanıcı açıklamayı okuyup dokununca) (+ `uses-feature android.hardware.camera required=false`: kamerasız cihazda kurulur, kod yapıştırılır; bir de androidx.core'un her uygulamada tanımladığı, uygulamaya özel imza düzeyi `<paket>.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION`; sistem izni değildir); `allowBackup=false`, `fullBackupContent=false`, `dataExtractionRules` tümünü dışlar; açılış zemini `#ECEEF2` / gece `#0F1115`. Geliştirme paketi (`--debug`, `pnpm tauri android dev`) yalnız Vite sunucusu için `usesCleartextTraffic=true` taşır (`src/debug/AndroidManifest.xml`); sürüm paketinde yoktur.

### Arka plan: Tauri yaşam döngüsü ve anlık bildirim

Araştırma (Tauri 2.12.3, tao 0.37.1, wry 0.57.0 kaynakları; `gen/android/.../generated/*.kt`):

1. **Başlatma süreç başına bir kez.** `WryActivity.onCreate` `ProcessLifecycleOwner`a `WryLifecycleObserver` ekler; onun `onCreate`i (süreçte bir kez) `Rust.onFirstActivityCreate` → tao `ndk_glue::onFirstActivityCreate` → ayrı iş parçacığında `run()`. `setup` (çekirdek, eşitleyiciler, eklenti kaydı) süreç başına bir kez çalışır; etkinlik yeniden yaratılınca tekrar çalışmaz (çift başlatma yok). Yapılandırma değişikliğinde (döndürme vb.) tao/wry pencere ve webview'u korur.
2. **Etkinlik arka planda (Ana ekran, başka uygulama, Android 12+ kökte geri).** `onPause/onStop` yalnız `Suspended/Stopped` olayı üretir; tokio çalışma ortamı (`mh_app::logging::runtime`, ayrı iş parçacıkları), IMAP IDLE, JMAP EventSource, `AppHandle`, eklenti tutamaçları yaşar, bildirim üretilebilir. Ön plan hizmeti yoksa süreç "önbellekte" sayılır: Android 11+ önbellek dondurucusu süreci kısa sürede dondurur (iş parçacıkları durur, IDLE akmaz), üretici uyutucuları ve bellek baskısı süreci öldürür.
3. **Etkinlik yok edilince (Android ≤ 11'de kökte geri, son uygulamalardan kaydırma, `finish`).** tao `WindowEvent::Destroyed` → tauri-runtime-wry pencereyi siler; pencere kalmayınca `RunEvent::ExitRequested { code: None }`, engellenmezse `ControlFlow::Exit` → `std::process::exit`. Süreç ve içindeki hizmet kapanır. Çıkış engellense bile çekirdek yaşayamaz: wry `onWebviewDestroy` etkinlik vekilini siler, sonraki her `run_mobile_plugin` (kasa `secretGet`, bildirim, dosya) `dispatch`te `first_activity_id().expect("no available activity")` ile panikler (sürüm profili `panic = "abort"`: süreç çöker); eklenti örnekleri ve Tauri'nin geri işleyicisi ilk etkinliğe bağlıdır; `setup` yeniden çalışmadığından yeni etkinlikte webview kurulmaz (boş ekran).
4. **Etkinliksiz başlatma (açılış alıcısı, WorkManager, bildirim düğmesi).** `.so` ve `Rust` sınıfı yalnız etkinlikle yüklenir; `setup` → `register_android_plugin` etkinlik bağlamı ister, `create_main` `NoAvailableActivity` döner. `mh_app::state::Core` Tauri'siz kurulabiliyor ama kasa (`SecretVault`) ve bildirim eklenti köprüsünden geçer; etkinlik açılınca `setup_core` ikinci bir `Core` kurar (çift eşitleme).

Karar:

- **"Anlık bildirim" (isteğe bağlı, varsayılan kapalı):** Ayarlar › Bildirimler anahtarı → `ArkaPlanHizmeti` ön plan hizmeti, kanal `arka-plan` (düşük önem, sessiz, rozetsiz; "Kapat" düğmeli). Hizmetin kendi işi yoktur; süreci "ön plan hizmeti" önceliğinde tutar: dondurulmaz, Doze'da ağ kesilmez, uyutuculara karşı dayanıklıdır. Tür `specialUse` (Android 14+; alt tür beyanı manifestte): `dataSync` Android 15'te 24 saatte 6 saatle sınırlı ve süre dolunca durdurulur, `BOOT_COMPLETED`ten de başlatılamaz; `remoteMessaging` cihazlar arası mesajlaşma içindir. Play'e çıkılırsa `specialUse` gerekçesi Play Console'da beyan edilir. Hizmet `MainActivity.onResume`da (ekrandayken; Android 12+ arka plandan başlatmaya izin vermez) ve anahtar açılınca başlar; `stopWithTask=true` (görev kaydırılınca durur: çekirdek etkinlikle yaşar), `START_NOT_STICKY` (sistem öldürürse çekirdeksiz yeniden doğmaz). Açılışta (`BOOT_COMPLETED`) başlatılmaz: çekirdek etkinliksiz açılamaz; telefon yeniden başlayınca bağlantı uygulama ilk açıldığında kurulur (arayüz bunu söyler).
- **Uyanma alarmı:** hizmet açıkken 15 dakikada bir `setAndAllowWhileIdle(ELAPSED_REALTIME_WAKEUP)` (kesin olmayan; izin gerektirmez, Doze'da da en çok ~9–15 dk'da bir). Alıcı yayını 8 s açık tutar (yayın uyanıklık kilidi; `WAKE_LOCK` izni gerekmez) ve çekirdeğe `uyan` gönderir → `sync_now` (IMAP + JMAP): uykuda yarı ölü kalmış TCP / NAT zaman aşımı bir eşitlemeyle ortaya çıkar, motor yeniden bağlanır. Rust zamanlayıcıları `CLOCK_MONOTONIC` kullandığından derin uykuda ilerlemez; `mh_app::wake::sleep_watch` uyanınca boşluğu görüp bağlantıları yeniler.
- **Kökte geri (Android 7–11):** `MainActivity.onBackPressed` görevi arka plana alır (Android 12+ davranışı); etkinlik yok edilip süreç (ve anlık bildirim) kapanmaz. Arayüzün geri yığını önce boşalır.
- **Pil:** "Pil kısıtlaması" satırı durumu gösterir, "Kısıtlamayı kaldır" açıklamayla birlikte sistem isteğini açar; üretici uyutucuları için kısa not (bağlantı yok).
- **Aşama 7 (WorkManager, açılışta başlatma) — yapılmadı, gereksinimler:** (a) kabukta JNI giriş noktası ile etkinliksiz çekirdek (`Core::open` + Tauri'siz `EventSink`), (b) kasa ve bildirim için eklentisiz JNI köprüsü (uygulama sınıf yükleyicisinin `GlobalRef`i ile `SecretVault`, `Bildirimler`), (c) mh-app'te `setup_core`un var olan `Core`u devralması (süreç başına tek çekirdek, çift eşitleme yok), (d) `Platform` çağrılarının pencere yokken güvenli olması. JMAP için sunucusuz seçenek UnifiedPush (`PushSubscription`).

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

`CI` iş akışı (push main + PR): alt modül recursive; pnpm + Node 22; kilit ve sürüm denetimi; `pnpm --filter app check/test`; Rust stable + `aarch64-linux-android`; Java 21; NDK 28.2 (yoksa sdkmanager); `cargo fmt --check`; Android clippy (`-D warnings`); bağımlılık ağacında `keyring`, `tauri-plugin-updater`, `tauri-plugin-single-instance` yok; host clippy + `cargo test`; debug APK; `scripts/apk-denetle.sh` (paket adı, izinler INTERNET/POST_NOTIFICATIONS/CAMERA/FOREGROUND_SERVICE(_SPECIAL_USE)/REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, kamera özelliği isteğe bağlı, minSdk 24, arm64-v8a, manifest, QrScanActivity ve BildirimAlici dışa kapalı, ArkaPlanHizmeti dışa kapalı + specialUse + stopWithTask, dex'te Play Hizmetleri / ML Kit yok ve zxing var, R8 sonrası eklenti komutları dex'te, 16 KB hizası); APK 14 gün artifact.

## 6. Cihaz denetim listesi

Bu ortamda cihaz/emülatör çalıştırılamadığından aşağıdakiler gerçek cihazda doğrulanır (her sürümde, en az Android 7 ve Android 14+ birer cihaz):

1. **Açılış:** ilk açılışta beyaz parlama yok (açık/koyu tema zemin rengi); arayüz yüklenir; `adb logcat -s Mailhere` "veri klasörü" satırını gösterir.
2. **Kasa:** hesap ekle → uygulamayı kapat/aç → parola sorulmadan eşitlenir (`mailhere-kasa.xml` var, düz metin içermez). Hesabı kaldır → kayıt silinir. StrongBox'lı ve StrongBox'sız cihazda anahtar üretimi.
3. **Veritabanı:** `files/mailhere.db` SQLCipher ile şifreli (`strings` ile okunamaz); uygulama verisi `adb backup` ile çıkmaz (`allowBackup=false`).
4. **Bildirim:** Android 13+'ta izin isteği (`notification_permission`); yeni postada kanal `yeni-posta` bildirimi (simge zarf, renk vurgu); başlık gönderen, alt metin hesap, gövde konu, genişletince önizlemenin ilk satırı; aynı anda 3 posta → tek ses, en yenisi üstten görünür, hesap özeti "3 yeni posta"; iki hesap → iki grup; dokununca ilgili posta açılır (`mh:open`; uygulama kapalıyken de, sayfa yüklenince) ve bildirim kalkar; postayı uygulamada ya da bilgisayarda okuyunca bildirim birkaç saniyede kalkar, özet sayısı düşer; kilit ekranında "hassas içeriği gizle" açıkken yalnız hesap adı ve "Yeni posta"; Elek'e düşen göndericiden bildirim gelmez; sessize alma ve hesap başına bildirim kapatma çalışır.
4a. **Bildirim eylemleri:** Okundu → bildirim kalkar, posta okunmuş (bilgisayarda da); Arşivle → posta Arşiv'de; uçak kipinde Arşivle → bildirim kalkar, bağlantı gelince sunucuda uygulanır; satır içi Yanıtla → geri sayımlı "Geri al" satırı, süre bitince yanıt Gönderilenler'de (konu `Re:`, alıntı, imza); Geri al → "Yanıt geri alındı". Uygulamayı son uygulamalardan kaydırıp bildirimde Arşivle → "Mailhere açılınca uygulanacak"; uygulama açılınca uygulanır ve bildirim kalkar. Wear OS / Android Auto'da eylemler görünür.
4b. **Anlık bildirim:** Ayarlar › Bildirimler › "Anlık bildirim" aç → bildirim çubuğunda sessiz "Anlık bildirim açık" (kanal "Arka plan bağlantısı"); Ana ekran tuşu + ekran kapalı 30 dk → başka cihazdan gönderilen posta saniyeler içinde bildirilir (`adb shell dumpsys deviceidle force-idle` ile Doze'da da); "Kapat" düğmesi ve anahtar hizmeti durdurur; uygulamayı yeniden açınca anahtar açıksa hizmet geri gelir; görevi son uygulamalardan kaydırınca hizmet durur; Android 14/15/16'da `adb shell dumpsys activity services com.furkanyildirim.mailhere` türü `specialUse` gösterir.
4c. **Pil kısıtlaması:** varsayılan "Kısıtlamayı kaldır" → Android'in onay penceresi → "Yok"; pil kullanımı "Kısıtlı" iken uygulama ayar sayfası açılır, "Kısıtlamasız" seçip dönünce satır güncellenir.
4d. **Dokunsal geri bildirim:** kaydırma eşiğinde hafif tık, onayda/redde farklı titreşim; sistem "dokunma geri bildirimi" kapalıyken titremez.
5. **Ekler:** eki aç (PDF/resim uygun uygulamada, FileProvider adresi `content://com.furkanyildirim.mailhere.ekler/ekler/...`); güvenli olmayan tür paylaşım sayfasına düşer; "Paylaş" (`attachment_share`) ve "Kaydet" (`attachment_save`, SAF) çalışır; ek eklerken seçilen dosya önbelleğe kopyalanır ve gönderilen postada adı/içeriği doğru gider.
6. **Yazma ve okuma:** `open_compose` → `mh:navigate` ile yazma yığını açılır; `.eml` kaydetme SAF ile; takvim daveti `.ics` takvim uygulamasında açılır.
7. **Sertifika onayı:** kendinden imzalı sunucuda onay iletişim kutusu (ebeveynsiz `confirm`) görünür, ret/onay doğru işler.
8. **Açılış hatası:** kasa erişilemez durumdayken (örn. test için kasa kaydı bozulmuş) hata iletişim kutusu gösterilir, "Kapat" uygulamayı kapatır; dosya silinmez.
9. **OAuth:** Google/Microsoft girişi tarayıcıda açılır, `127.0.0.1` dönüşü uygulamaya gelir (cihazda doğrulanmalı).
10. **Ağ:** uçak kipi → kapat → eşitleme kendiliğinden toparlar; IDLE/JMAP anlık bildirim arka planda kesilince yeniden kurulur (anlık bildirim açıkken; kapalıyken süreç dondurulabilir, uygulama açılınca toparlar).
11. **Geri tuşu:** mobil arayüz geldiğinde çekmece/alt sayfa/yığın sırası; kökte uygulama arka plana gider (Android 7–11'de de; süreç ve anlık bildirim yaşar).
12. **Performans:** 10 bin postalı hesapta liste kaydırması akıcı; soğuk açılış < 2 s (orta sınıf cihaz).
13. **16 KB sayfa:** Android 15+ 16 KB cihaz/emülatörde kurulup açılır (`scripts/apk-denetle.sh` hizayı doğrular).
14. **Güncelleme:** önceki sürümün üstüne kurulum veriyi korur; `versionCode` artmış olmalı.
15. **QR aktarım — izin:** "Bilgisayardan al" ilk kez açılınca kamera izni sorulur; reddedilince "Kamera izni verilmedi; Ayarlar'dan izin ver ya da kodu yapıştır." görünür ve kod yapıştırma yolu çalışır; "Bir daha sorma" sonrasında sistem iletişim kutusu çıkmadan aynı hata gelir. Kamerasız cihaz/emülatörde "Kamera açılamadı…" hatası ve yapıştırma yolu.
16. **QR aktarım — tarayıcı:** koyu tam ekran, önizleme akıcı, çerçeve kılavuzu ortada, sistem çubukları saydam (çentik/geçişli ekranda üst çubuk güvenli alanda); geri düğmesi ve sistem geri tuşu `null` ile kapatır; fener düğmesi yalnız flaşlı cihazda görünür, açınca vurgu rengine döner, ekran kapanınca söner; döndürünce ekran yeniden oluşturulmaz ve okuma sürer; başka bir QR'da (ör. web adresi) "Bu bir Mailhere aktarım kodu değil" 1,8 s görünür, tarama devam eder; masaüstündeki QR ters (koyu tema) gösterilirken de okunur.
17. **QR aktarım — uçtan uca:** bilgisayarda Ayarlar › Cihazlar › "Telefona aktar" → telefonda QR okut → özet (hesaplar, kurallar, etiketler, ayarlar) → uygula → bilgisayarda "uygulandı" durumu → telefonda bildirim izni isteği (13+) → gelen kutusu eşitlenir; parolasız hesap raporda listelenir. Aynı Wi-Fi'de değilken (hücresel veri) 30 s içinde anlaşılır ağ hatası; kodu yapıştırarak alma aynı sonucu verir; 3 dk sonra süresi dolan kod reddedilir.

## 7. Bilinen sınırlar (Aşama 4d)

- Arayüzün `qr_scan`, `notification_permission`, `attachment_share`, `attachment_save` komutlarını sözleşmeye (`app/src/lib/api/types.ts`) ve sahte arka uca eklemesi ile "Bilgisayardan al" ekranının tarayıcıyı çağırması alt modülün Aşama 4c işidir; kabuk tarafı hazırdır.
- Telefondan bilgisayara aktarım (telefonda `transfer_begin`) henüz arayüzde yok; komut ortak listede olduğundan izinlidir.
- Arka plan eşitlemesi (WorkManager) ve açılışta (`BOOT_COMPLETED`) bağlanma Aşama 7: Rust çekirdeği etkinliksiz başlatılamaz (bkz. §3 "Arka plan"). Anlık bildirim kapalıyken uygulama arka planda dondurulabilir; açıkken son uygulamalardan kaydırma ve telefonu yeniden başlatma bağlantıyı uygulama açılana dek keser.
- Süreç kapalıyken basılan bildirim düğmesi uygulama açılınca uygulanır; o arada başka cihazdan yapılan değişiklikle çakışırsa son işlem geçerlidir. Süreç kapalıyken "Geri al" çoğu zaman geç kalır (giden kutusu açılışta hemen gönderir); bildirim sonucu söyler.
- Bildirim metinleri (kanal adları, düğmeler, özet) telefonun dilini izler (`strings.xml`); arayüzün dil ayarını değil.
