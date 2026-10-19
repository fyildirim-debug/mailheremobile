#!/usr/bin/env bash
# APK denetimi: paket adı, izinler, minSdk, ABI, bildirim öznitelikleri,
# QR tarayıcı (kamera isteğe bağlı, etkinlik dışa kapalı, Play Hizmetleri /
# ML Kit yok), bildirim alıcısı, anlık bildirim hizmeti (dışa kapalı,
# specialUse, görevle durmaz), açılış alıcısı, yoklama işi, yeniden başlatma
# trampolini (ayrı süreç), R8 sonrası eklenti komutları ve JNI köprüsü
# (Kotlin sınıfı dex'te, Rust girişleri .so'da) ve .so dosyasının 16 KB
# sayfa hizası. CI ve yerelde aynı betik.
#
#   scripts/apk-denetle.sh <apk> [--debug]
#
# --debug: geliştirme paketinde dev sunucusu için usesCleartextTraffic açıktır;
# sürüm paketinde bu öznitelik bulunmamalıdır.
set -euo pipefail

APK="${1:?APK yolu gerekli}"
DEBUG=0
[ "${2:-}" = "--debug" ] && DEBUG=1

SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/opt/android-sdk}}"
BT="$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)"
AAPT2="$BT/aapt2"
NDK="${NDK_HOME:-${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* 2>/dev/null | sort -V | tail -1)}}"
OBJDUMP="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-objdump"
[ -x "$OBJDUMP" ] || OBJDUMP="$(command -v llvm-objdump)"
NM="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-nm"
[ -x "$NM" ] || NM="$(command -v llvm-nm)"

hata=0
sorun() { echo "HATA: $*" >&2; hata=1; }

BADGING="$("$AAPT2" dump badging "$APK")"
echo "$BADGING" | grep -E "^ *(package|sdkVersion|targetSdkVersion|uses-permission|uses-feature|uses-feature-not-required|native-code|application-label):" || true

echo "$BADGING" | grep -q "^package: name='com.furkanyildirim.mailhere'" || sorun "paket adı com.furkanyildirim.mailhere değil"
echo "$BADGING" | grep -Eq "^(minS|s)dkVersion:'24'" || sorun "minSdk 24 değil"
echo "$BADGING" | grep -q "^targetSdkVersion:'36'" || sorun "targetSdk 36 değil"
echo "$BADGING" | grep -q "^native-code: 'arm64-v8a'$" || sorun "yalnız arm64-v8a beklenir"
echo "$BADGING" | grep -q "^application-label:'Mailhere'" || sorun "uygulama adı Mailhere değil"

# Uygulamanın kendi tanımladığı imza düzeyi izni (androidx.core, dışa açık
# olmayan alıcılar için) sistem izni değildir; sayılmaz.
IZINLER="$(echo "$BADGING" | sed -n "s/^uses-permission: name='\([^']*\)'.*/\1/p" | grep -v '^com.furkanyildirim.mailhere.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION$' | sort)"
# Anlık bildirim (varsayılan açık): ön plan hizmeti (specialUse), kullanıcı
# dokununca istenen pil optimizasyonu muafiyeti, açılışta hizmet ve kalıcı
# yoklama işi için RECEIVE_BOOT_COMPLETED. Sunucu itmesi (Firebase Cloud
# Messaging): WAKE_LOCK (itmede eşitleme bitene dek, en çok 30 sn),
# c2dm RECEIVE (Google Play Hizmetleri'nden ileti) ve ACCESS_NETWORK_STATE
# (firebase-messaging). Kesin alarm, konum, kişiler yok.
BEKLENEN="$(printf '%s\n' android.permission.INTERNET android.permission.POST_NOTIFICATIONS android.permission.CAMERA \
  android.permission.FOREGROUND_SERVICE android.permission.FOREGROUND_SERVICE_SPECIAL_USE \
  android.permission.REQUEST_IGNORE_BATTERY_OPTIMIZATIONS android.permission.RECEIVE_BOOT_COMPLETED \
  android.permission.WAKE_LOCK android.permission.ACCESS_NETWORK_STATE com.google.android.c2dm.permission.RECEIVE | sort)"
if [ "$IZINLER" != "$BEKLENEN" ]; then
  sorun "izinler beklenen listeden farklı (INTERNET, POST_NOTIFICATIONS, CAMERA, FOREGROUND_SERVICE(_SPECIAL_USE), REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, RECEIVE_BOOT_COMPLETED, WAKE_LOCK, ACCESS_NETWORK_STATE, c2dm RECEIVE); bulunan: $(echo "$IZINLER" | tr '\n' ' ')"
fi
# Kamera yalnız QR tarayıcı için; kamerasız cihazlarda da kurulabilmeli
# (badging satırı feature-group altında girintilidir).
echo "$BADGING" | grep -Eq "^ *uses-feature-not-required: name='android.hardware.camera'" || sorun "android.hardware.camera required=false değil"

MANIFEST="$("$AAPT2" dump xmltree --file AndroidManifest.xml "$APK")"
echo "$MANIFEST" | grep -q 'android:allowBackup.*=false' || sorun "allowBackup=false değil"
echo "$MANIFEST" | grep -q 'android:dataExtractionRules' || sorun "dataExtractionRules yok"
echo "$MANIFEST" | grep -q 'android:fullBackupContent.*=false' || sorun "fullBackupContent=false değil"
echo "$MANIFEST" | grep -q 'android:authorities.*="com.furkanyildirim.mailhere.ekler"' || sorun "FileProvider (.ekler) yok"
echo "$MANIFEST" | grep -q 'LocalNotificationRestoreReceiver' && sorun "açılışta geri yükleme alıcısı atılmamış"
# QR tarayıcı etkinliği kayıtlı ve dışa kapalı (yalnız MhPlugin açar).
QR="$(echo "$MANIFEST" | awk '/^ *E: / { if (blok ~ /QrScanActivity/) print blok; blok = "" } { blok = blok "\n" $0 } END { if (blok ~ /QrScanActivity/) print blok }')"
echo "$MANIFEST" | grep -q 'com.furkanyildirim.mailhere.plugin.QrScanActivity' || sorun "QrScanActivity manifestte yok"
echo "$QR" | grep -q 'android:exported.*=false' || sorun "QrScanActivity exported=false değil"
# Bir öğenin (E: satırından sonraki öznitelikler) bloğu.
blok() {
  echo "$MANIFEST" | awk -v ad="$1" '/^ *E: / { if (b ~ ad) print b; b = "" } { b = b "\n" $0 } END { if (b ~ ad) print b }'
}
# Bir öğenin alt ağacı: öğeden sonra, aynı ya da daha az girintili bir sonraki
# E: satırına kadar (niyet süzgeçleri ve eylemleri de kapsar).
agac() {
  echo "$MANIFEST" | awk -v ad="$1" '
    /^ *E: / { g = match($0, /E:/) }
    acik && /^ *E: / && g <= derin { acik = 0 }
    acik { print; next }
    /^ *E: / { aday = $0; adayg = g; next }
    aday != "" && $0 ~ ad { print aday; print; acik = 1; derin = adayg; aday = "" }
  '
}
# Bildirim düğmeleri alıcısı ve anlık bildirim hizmeti dışa kapalı; hizmet
# specialUse türünde (alt tür beyanıyla) ve görev kapanınca durmaz (çekirdek
# etkinliksiz yaşar).
ALICI="$(blok 'plugin.BildirimAlici')"
[ -n "$ALICI" ] || sorun "BildirimAlici manifestte yok"
echo "$ALICI" | grep -q 'android:exported.*=false' || sorun "BildirimAlici exported=false değil"
echo "$ALICI" | grep -q 'intent-filter' && sorun "BildirimAlici niyet süzgeci taşımamalı"
HIZMET="$(blok 'plugin.ArkaPlanHizmeti')"
[ -n "$HIZMET" ] || sorun "ArkaPlanHizmeti manifestte yok"
echo "$HIZMET" | grep -q 'android:exported.*=false' || sorun "ArkaPlanHizmeti exported=false değil"
echo "$HIZMET" | grep -q 'android:foregroundServiceType.*=0x40000000' || sorun "ArkaPlanHizmeti türü specialUse değil"
echo "$HIZMET" | grep -q 'android:stopWithTask.*=false' || sorun "ArkaPlanHizmeti stopWithTask=false değil"
echo "$MANIFEST" | grep -q 'PROPERTY_SPECIAL_USE_FGS_SUBTYPE' || sorun "specialUse alt tür beyanı yok"
# Uygulama sınıfı (JNI köprüsünü kurar), açılış alıcısı (yalnız sistem
# yayınları), yoklama işi (yalnız sistem bağlanır) ve ayrı süreçteki trampolin.
echo "$MANIFEST" | grep -q 'com.furkanyildirim.mailhere.MailhereApp' || sorun "MailhereApp uygulama sınıfı manifestte yok"
BOOT="$(agac 'plugin.BootAlici')"
[ -n "$BOOT" ] || sorun "BootAlici manifestte yok"
echo "$BOOT" | grep -q 'android:exported.*=true' || sorun "BootAlici exported=true değil (sistem yayını alamaz)"
echo "$BOOT" | grep -q 'android.intent.action.BOOT_COMPLETED' || sorun "BootAlici BOOT_COMPLETED süzgeci yok"
echo "$BOOT" | grep -q 'android.intent.action.MY_PACKAGE_REPLACED' || sorun "BootAlici MY_PACKAGE_REPLACED süzgeci yok"
IS="$(blok 'plugin.ArkaPlanIsi')"
[ -n "$IS" ] || sorun "ArkaPlanIsi manifestte yok"
echo "$IS" | grep -q 'android:exported.*=false' || sorun "ArkaPlanIsi exported=false değil"
echo "$IS" | grep -q 'android.permission.BIND_JOB_SERVICE' || sorun "ArkaPlanIsi BIND_JOB_SERVICE izni yok"
# Sunucu itmesi hizmeti dışa kapalı, yalnız Firebase iletisi süzgeci; Analytics kapalı.
ITME="$(agac 'plugin.ItmeServisi')"
[ -n "$ITME" ] || sorun "ItmeServisi manifestte yok"
echo "$ITME" | grep -q 'android:exported.*=false' || sorun "ItmeServisi exported=false değil"
echo "$ITME" | grep -q 'com.google.firebase.MESSAGING_EVENT' || sorun "ItmeServisi MESSAGING_EVENT süzgeci yok"
echo "$MANIFEST" | grep -q 'firebase_analytics_collection_deactivated' || sorun "Firebase Analytics kapatılmamış"
YENIDEN="$(blok 'YenidenBaslatActivity')"
[ -n "$YENIDEN" ] || sorun "YenidenBaslatActivity manifestte yok"
echo "$YENIDEN" | grep -q 'android:exported.*=false' || sorun "YenidenBaslatActivity exported=false değil"
echo "$YENIDEN" | grep -q 'android:process.*=":yeniden"' || sorun "YenidenBaslatActivity ayrı süreçte (:yeniden) değil"
echo "$YENIDEN" | grep -q 'android:excludeFromRecents.*=true' || sorun "YenidenBaslatActivity excludeFromRecents=true değil"
if [ "$DEBUG" = 1 ]; then
  echo "$MANIFEST" | grep -q 'android:usesCleartextTraffic.*=true' || echo "Not: geliştirme paketinde usesCleartextTraffic beklenirdi"
else
  echo "$MANIFEST" | grep -q 'android:usesCleartextTraffic' && sorun "sürüm paketinde usesCleartextTraffic olmamalı"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Temiz oda: QR çözme zxing core ile, ML Kit yok. Google Play Hizmetleri yalnız
# sunucu itmesi için (firebase-messaging: common, tasks, cloudmessaging);
# ölçüm/Analytics, Crashlytics, reklam, konum, harita ve görüntü sınıfı yok.
unzip -q -o "$APK" 'classes*.dex' -d "$TMP/dex"
# (firebase/analytics/connector yalnız boş bir arayüzdür; Analytics'in kendisi
# FirebaseAnalytics ve gms/measurement'tır.)
if grep -l -a -E 'Lcom/google/(mlkit/|android/gms/(measurement|analytics|ads|location|maps|vision|ads_identifier)/|firebase/crashlytics/|firebase/analytics/FirebaseAnalytics;)' "$TMP"/dex/classes*.dex > /dev/null 2>&1; then
  sorun "dex'te ML Kit, ölçüm, reklam, konum ya da görüntü sınıfı var"
fi
grep -l -a 'Lcom/google/firebase/messaging/FirebaseMessagingService;' "$TMP"/dex/classes*.dex > /dev/null 2>&1 || sorun "dex'te Firebase Messaging yok"
grep -l -a 'Lcom/google/zxing/qrcode/QRCodeReader;' "$TMP"/dex/classes*.dex > /dev/null 2>&1 || sorun "dex'te zxing QRCodeReader yok"
# R8 sonrası: Rust'ın adla çağırdığı eklenti komutları, JNI köprüsünün
# statik yöntemleri (proguard-rules.pro korur) ve manifestteki bileşenler
# küçültmeden sağ çıkmış olmalı (Tauri @Command kuralı korur).
for ad in Lcom/furkanyildirim/mailhere/plugin/MhPlugin\; Lcom/furkanyildirim/mailhere/plugin/BildirimAlici\; \
  Lcom/furkanyildirim/mailhere/plugin/ArkaPlanHizmeti\; Lcom/furkanyildirim/mailhere/plugin/ArkaPlanIsi\; \
  Lcom/furkanyildirim/mailhere/plugin/BootAlici\; Lcom/furkanyildirim/mailhere/plugin/ArkaKopru\; \
  Lcom/furkanyildirim/mailhere/MailhereApp\; Lcom/furkanyildirim/mailhere/YenidenBaslatActivity\; \
  pilDurumu pilAyarlari arkaPlanDurumu arkaPlanAyarla acilisDurumu acilisAyarla haptic openNotificationSettings \
  kasaOku kasaYaz kasaSil yanitKuyrukta yanitGeriAlindi bildirimlerAcik bekleyenler anlikAcik kilitliGoster kilitliKaldir \
  cekirdekBaslat cekirdekDurdur cekirdekDurumu tekSefer; do
  grep -l -a -F "$ad" "$TMP"/dex/classes*.dex > /dev/null 2>&1 || sorun "dex'te $ad yok (R8 sildi mi?)"
done
# Ön plan hizmetinin kanalı en düşük önemle yeni kimlikte (durum çubuğunda
# simge yok); kanal önemi sonradan değişmediği için eski kimlik silinir.
grep -l -a -F 'arka-plan-sessiz' "$TMP"/dex/classes*.dex > /dev/null 2>&1 || sorun "dex'te arka plan kanalı arka-plan-sessiz yok"

# 16 KB sayfa hizası (Android 15+): her LOAD bölütü en az 2**14 hizalı.
unzip -q -o "$APK" 'lib/arm64-v8a/*.so' -d "$TMP"
SO_SAYISI=0
for so in "$TMP"/lib/arm64-v8a/*.so; do
  SO_SAYISI=$((SO_SAYISI + 1))
  HIZA="$("$OBJDUMP" -p "$so" | awk '/^ *LOAD/ { for (i = 1; i <= NF; i++) if ($i ~ /^2\*\*/) print substr($i, 4) }')"
  for h in $HIZA; do
    if [ "$h" -lt 14 ]; then sorun "$(basename "$so"): LOAD hizası 2**$h (< 2**14)"; fi
  done
  echo "$(basename "$so"): LOAD hizaları $(echo "$HIZA" | tr '\n' ' ')"
done
[ "$SO_SAYISI" -ge 1 ] || sorun "APK'da arm64-v8a .so yok"
ls "$TMP"/lib/arm64-v8a/ | grep -q '^libmailhere_lib.so$' || sorun "libmailhere_lib.so yok"

# JNI köprüsü: Kotlin ArkaKopru'nun her external yöntemi .so'da dışa verilmiş
# bir Java_… sembolüyle eşleşir (etkinliksiz çekirdek bunlarla açılır); Tauri'nin
# kendi girişi de yerinde.
if [ -x "$NM" ]; then
  SEMBOLLER="$("$NM" -D --defined-only "$TMP"/lib/arm64-v8a/libmailhere_lib.so 2>/dev/null | awk '{ print $NF }')"
  for ad in kur cekirdekBaslat cekirdekDurdur cekirdekDurumu esitle eylem tekSefer push; do
    echo "$SEMBOLLER" | grep -qx "Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_$ad" \
      || sorun ".so'da Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_$ad yok"
  done
  echo "$SEMBOLLER" | grep -qx 'Java_com_furkanyildirim_mailhere_Rust_onFirstActivityCreate' \
    || sorun ".so'da Tauri girişi (Rust.onFirstActivityCreate) yok"
  echo "JNI girişleri: $(echo "$SEMBOLLER" | grep -c '^Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_') ArkaKopru sembolü"
else
  sorun "llvm-nm bulunamadı; JNI sembolleri denetlenemedi"
fi

if [ "$hata" = 0 ]; then echo "APK denetimi geçti: $APK"; else exit 1; fi
