# Mailhere Android

[Mailhere](https://github.com/fyildirim-debug/mailhere) e-posta istemcisinin Android kabuğu. Çekirdek (IMAP / JMAP eşitleme, şifreli yerel depo, kurallar, gönderim) ve arayüz masaüstüyle aynı koddan gelir; bu depo yalnız Android'e özgü parçayı taşır: tek pencere, `AndroidKeyStore` destekli parola kasası, sistem bildirimi, dosya seçme / kaydetme / paylaşma ve Gradle projesi.

- Ortak kod `mailhere/` git alt modülüdür (sabit commit). Ortak kod orada geliştirilir; burada yalnız kabuk değişir.
- Paket adı `com.furkanyildirim.mailhere`; Android 7.0 (API 24) ve üstü, yalnız arm64-v8a.
- Parolalar ve veritabanı anahtarı donanım destekli kasada; veritabanı diskte şifreli (SQLCipher); uygulama verisi yedeklemeye kapalı.

## Hızlı başlangıç

```bash
git clone --recurse-submodules https://github.com/fyildirim-debug/mailheremobile
cd mailheremobile
pnpm install
pnpm tauri android dev                       # cihaz ya da emülatör
pnpm tauri android build --apk --target aarch64
```

Kurulum ayrıntıları (Android SDK, NDK 28.2, Java 21), mimari, sürüm şeması ve cihaz denetim listesi: [`docs/ANDROID.md`](docs/ANDROID.md). Çalışma kuralları: [`CLAUDE.md`](CLAUDE.md).

## Lisans

MIT — bkz. [`LICENSE`](LICENSE).
