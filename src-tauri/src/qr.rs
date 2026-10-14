//! QR tarayıcı: `qr_scan` komutu Android'de tam ekran `QrScanActivity`'yi
//! (`gen/android/app/src/main/java/.../plugin/QrScanActivity.kt`; CameraX
//! önizleme + zxing core çözme, ikisi Apache-2.0) açar ve okunan aktarım
//! kodunu döner. Kamera görüntüsü cihazda kalır; ML Kit ya da Google Play
//! Hizmetleri kullanılmaz.
//!
//! Kotlin sözleşmesi (`MhPlugin.scanQr`): `{}` → `{ text: string | null }`
//! (vazgeçince `null`); kamera izni verilmediyse `kamera-izni`, kamera
//! açılamadıysa `kamera` iletisiyle reddedilir. Masaüstündeki geliştirme
//! ikilisinde kamera yoktur: `Ok(None)` döner, arayüz kodu yapıştırma
//! yoluna düşer.

use mh_core::{AppError, ErrorCode};
use serde::Deserialize;
use tauri::AppHandle;

type Res<T> = Result<T, AppError>;

/// Kotlin `scanQr` yanıtı.
#[derive(Deserialize)]
struct ScanResponse {
    text: Option<String>,
}

/// Yalnız bu önekle başlayan metin aktarım kodudur (`mh-transfer`
/// `code.rs` ile aynı). Kotlin tarafı da yalnız bunu döndürür; burada
/// ikinci kez süzülür ki sözleşme tek yerde değişse bile arayüze başka bir
/// QR'ın metni ulaşmasın.
pub const PREFIX: &str = "mailhere://aktar?";

/// Kotlin'in ret iletisini kullanıcıya gösterilecek hataya çevirir
/// (ne oldu + ne yapılabilir).
pub fn scan_error(raw: &str) -> AppError {
    if raw.contains("kamera-izni") {
        AppError::new(
            ErrorCode::Internal,
            "Kamera izni verilmedi; Ayarlar'dan izin ver ya da kodu yapıştır.",
        )
    } else {
        tracing::warn!(error = %raw, "QR tarayıcı açılamadı");
        AppError::new(
            ErrorCode::Internal,
            "Kamera açılamadı. Kodu bilgisayardan kopyalayıp yapıştırabilirsin.",
        )
    }
}

/// Okunan metni süzer: boş ya da önek dışı metin `None`.
pub fn accept(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_owned())
        .filter(|t| t.starts_with(PREFIX))
}

/// Tam ekran QR tarayıcıyı açar; okunan kod, vazgeçilirse `None`.
#[tauri::command]
pub async fn qr_scan(app: AppHandle) -> Res<Option<String>> {
    if !cfg!(target_os = "android") {
        tracing::info!("QR tarayıcı yalnız Android'de; kod yapıştırılabilir");
        return Ok(None);
    }
    // Eklenti çağrısı tarayıcı kapanana dek bekler; çalışma zamanının iş
    // parçacığı engellenmesin.
    let res = tokio::task::spawn_blocking(move || {
        crate::plugin::call::<ScanResponse>(&app, "scanQr", serde_json::json!({}))
    })
    .await
    .map_err(mh_app::state::internal)?;
    match res {
        Ok(r) => Ok(accept(r.text)),
        Err(e) => Err(scan_error(&e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalniz_aktarim_kodu_kabul_edilir() {
        let kod = "mailhere://aktar?v=1&h=192.168.1.20&p=51234&t=oKGio6SlpqeoqaqrrK2urw&k=AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
        assert_eq!(accept(Some(format!("  {kod}\n"))).as_deref(), Some(kod));
        assert_eq!(accept(Some("https://ornek.com".into())), None);
        assert_eq!(accept(Some("MAILHERE://AKTAR?v=1".into())), None);
        assert_eq!(accept(Some(String::new())), None);
        assert_eq!(accept(None), None);
    }

    #[test]
    fn ret_iletisi_anlamli_hataya_doner() {
        let izin = scan_error("kamera-izni");
        assert_eq!(izin.code, ErrorCode::Internal);
        assert!(izin.message.contains("Ayarlar"));
        assert!(izin.message.contains("yapıştır"));
        let kamera = scan_error("[kamera] - kamera açılamadı");
        assert!(kamera.message.contains("Kamera açılamadı"));
        assert!(kamera.message.contains("yapıştır"));
    }
}
