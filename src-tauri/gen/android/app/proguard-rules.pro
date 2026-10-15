# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile
# Temiz oda denetimi (scripts/apk-denetle.sh): QR çözücünün zxing core olduğu
# sürüm paketinde de görülsün. Yalnız sınıf adı korunur; kullanılmazsa silinir.
-keepnames class com.google.zxing.qrcode.QRCodeReader
# Etkinliksiz JNI köprüsü (src/kopru.rs): Rust statik yöntemleri adla
# çağırır, Kotlin'in external yöntemleri .so içindeki Java_… sembollerine
# bağlanır; R8 adları ve sınıfı korur.
-keep class com.furkanyildirim.mailhere.plugin.ArkaKopru { *; }
