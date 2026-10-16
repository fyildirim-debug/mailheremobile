//! Açılışta başlama (Ayarlar › Genel): telefon açılınca `BootAlici` arka
//! plan hizmetini ve yoklama işini kursun mu. Bayrak Kotlin tercihindedir
//! (`ArkaPlan.acilistaBaslar`, `mailhere-arka-plan`; seçilmemişse açık).
//! Kapalıyken açılışta hiçbir şey başlatılmaz; uygulama açılınca anlık
//! bildirim ve yoklama işi her zamanki gibi çalışır.
//!
//! Komut adları masaüstüyle aynıdır (`autostart_get/set`), gövde kabuğa
//! özgüdür. Android kabuğunun masaüstü geliştirme ikilisinde hata döner
//! (arayüz satırı gizler).

use mh_core::{AppError, ErrorCode};
use serde::Deserialize;
use tauri::AppHandle;

type Res<T> = Result<T, AppError>;

#[derive(Deserialize)]
struct Response {
    acik: bool,
}

fn unsupported() -> AppError {
    AppError::new(ErrorCode::Invalid, "Bu ayar yalnız Android'de var.")
}

/// Açılışta başlama açık mı.
#[tauri::command]
pub async fn autostart_get(app: AppHandle) -> Res<bool> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    crate::arka::call::<Response>(app, "acilisDurumu", serde_json::json!({}))
        .await
        .map(|r| r.acik)
        .map_err(|e| {
            tracing::warn!(error = %e, "açılış ayarı okunamadı");
            AppError::new(
                ErrorCode::Internal,
                "Açılışta başlama ayarı okunamadı. Mailhere'yi kapatıp açtıktan sonra yeniden dene.",
            )
        })
}

/// Açılışta başlamayı açar ya da kapatır; son durumu döner. Yalnız ayardır:
/// çalışan hizmete dokunmaz, etkisi bir sonraki açılıştadır.
#[tauri::command]
pub async fn autostart_set(app: AppHandle, on: bool) -> Res<bool> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    crate::arka::call::<Response>(app, "acilisAyarla", serde_json::json!({ "acik": on }))
        .await
        .map(|r| r.acik)
        .map_err(|e| {
            tracing::warn!(error = %e, "açılış ayarı yazılamadı");
            AppError::new(
                ErrorCode::Storage,
                "Açılışta başlama ayarı kaydedilemedi. Yeniden dene; sürerse telefonda yer açıp Mailhere'yi yeniden başlat.",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotlin_yaniti() {
        let r: Response = serde_json::from_value(serde_json::json!({ "acik": false })).unwrap();
        assert!(!r.acik);
        assert!(serde_json::from_value::<Response>(serde_json::json!({})).is_err());
    }

    #[test]
    fn masaustunde_desteklenmez() {
        let e = unsupported();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Android"));
    }
}
