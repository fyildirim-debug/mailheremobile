#!/usr/bin/env bash
# APK denetimi: paket adı, izinler, minSdk, ABI, bildirim öznitelikleri ve
# .so dosyasının 16 KB sayfa hizası. CI ve yerelde aynı betik.
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

hata=0
sorun() { echo "HATA: $*" >&2; hata=1; }

BADGING="$("$AAPT2" dump badging "$APK")"
echo "$BADGING" | grep -E "^(package|sdkVersion|targetSdkVersion|uses-permission|native-code|application-label):" || true

echo "$BADGING" | grep -q "^package: name='com.furkanyildirim.mailhere'" || sorun "paket adı com.furkanyildirim.mailhere değil"
echo "$BADGING" | grep -Eq "^(minS|s)dkVersion:'24'" || sorun "minSdk 24 değil"
echo "$BADGING" | grep -q "^targetSdkVersion:'36'" || sorun "targetSdk 36 değil"
echo "$BADGING" | grep -q "^native-code: 'arm64-v8a'$" || sorun "yalnız arm64-v8a beklenir"
echo "$BADGING" | grep -q "^application-label:'Mailhere'" || sorun "uygulama adı Mailhere değil"

# Uygulamanın kendi tanımladığı imza düzeyi izni (androidx.core, dışa açık
# olmayan alıcılar için) sistem izni değildir; sayılmaz.
IZINLER="$(echo "$BADGING" | sed -n "s/^uses-permission: name='\([^']*\)'.*/\1/p" | grep -v '^com.furkanyildirim.mailhere.DYNAMIC_RECEIVER_NOT_EXPORTED_PERMISSION$' | sort)"
BEKLENEN="$(printf '%s\n' android.permission.INTERNET android.permission.POST_NOTIFICATIONS | sort)"
if [ "$IZINLER" != "$BEKLENEN" ]; then
  sorun "izinler yalnız INTERNET ve POST_NOTIFICATIONS olmalı; bulunan: $(echo "$IZINLER" | tr '\n' ' ')"
fi

MANIFEST="$("$AAPT2" dump xmltree --file AndroidManifest.xml "$APK")"
echo "$MANIFEST" | grep -q 'android:allowBackup.*=false' || sorun "allowBackup=false değil"
echo "$MANIFEST" | grep -q 'android:dataExtractionRules' || sorun "dataExtractionRules yok"
echo "$MANIFEST" | grep -q 'android:fullBackupContent.*=false' || sorun "fullBackupContent=false değil"
echo "$MANIFEST" | grep -q 'android:authorities.*="com.furkanyildirim.mailhere.ekler"' || sorun "FileProvider (.ekler) yok"
echo "$MANIFEST" | grep -q 'LocalNotificationRestoreReceiver' && sorun "açılışta geri yükleme alıcısı atılmamış"
if [ "$DEBUG" = 1 ]; then
  echo "$MANIFEST" | grep -q 'android:usesCleartextTraffic.*=true' || echo "Not: geliştirme paketinde usesCleartextTraffic beklenirdi"
else
  echo "$MANIFEST" | grep -q 'android:usesCleartextTraffic' && sorun "sürüm paketinde usesCleartextTraffic olmamalı"
fi

# 16 KB sayfa hizası (Android 15+): her LOAD bölütü en az 2**14 hizalı.
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
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

if [ "$hata" = 0 ]; then echo "APK denetimi geçti: $APK"; else exit 1; fi
