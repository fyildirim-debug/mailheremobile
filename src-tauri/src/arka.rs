//! Bildirim sağlığı: pil kısıtlaması ve isteğe bağlı "Anlık bildirim"
//! (sürekli bağlantı). Yalnız Android komutları; masaüstünde tanımlı değildir,
//! Android kabuğunun masaüstü geliştirme ikilisinde hata döner (arayüz
//! bölümü gizler).
//!
//! Anlık bildirim açıkken `ArkaPlanHizmeti` (ön plan hizmeti, tür
//! `specialUse`) süreci arka planda canlı tutar: Mailhere ekranı kapalıyken
//! de (Ana ekran tuşu, başka uygulamaya geçiş) IMAP IDLE ve JMAP anlık
//! bildirim bağlantıları sürer, süreç önbellek kipinde dondurulmaz ve Doze'da
//! ağ kesilmez. Hizmet görev kaydırılıp kapatılınca durur (`stopWithTask`);
//! Rust çalışma ortamı etkinlikle yaşar, etkinliksiz başlatılamaz
//! (bkz. `docs/ANDROID.md` "Arka plan").

use mh_core::{AppError, ErrorCode};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

type Res<T> = Result<T, AppError>;

/// Pil kısıtlaması.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Battery {
    /// Pil optimizasyonundan muaf: Doze ve üretici uyutması en az.
    Unrestricted,
    /// Varsayılan: Android arka plandaki uygulamayı uyutabilir.
    Optimized,
    /// Kullanıcı "Kısıtlı" seçmiş (Android 9+): arka planda çalışamaz.
    Restricted,
}

fn unsupported() -> AppError {
    AppError::new(ErrorCode::Invalid, "Bu ayar yalnız Android'de var.")
}

/// Eklenti çağrısı ana iş parçacığını bekler; çalışma iş parçacığını tutmasın.
pub(crate) async fn call<T: serde::de::DeserializeOwned + Send + 'static>(
    app: AppHandle,
    command: &'static str,
    payload: serde_json::Value,
) -> Result<T, String> {
    tokio::task::spawn_blocking(move || crate::plugin::call::<T>(&app, command, payload))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
struct BatteryResponse {
    durum: Battery,
}

#[derive(Deserialize)]
struct ModeResponse {
    acik: bool,
}

/// Pil kısıtlaması: `unrestricted` | `optimized` | `restricted`.
#[tauri::command]
pub async fn battery_status(app: AppHandle) -> Res<Battery> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    call::<BatteryResponse>(app, "pilDurumu", serde_json::json!({}))
        .await
        .map(|r| r.durum)
        .map_err(|e| {
            tracing::warn!(error = %e, "pil durumu okunamadı");
            AppError::new(
                ErrorCode::Internal,
                "Pil durumu okunamadı. Telefonun Ayarlar › Uygulamalar › Mailhere › Pil bölümüne bakabilirsin.",
            )
        })
}

/// Kısıtlamayı kaldırma: optimizasyondaysa sistemin "pil optimizasyonunu
/// yok say" isteği, "Kısıtlı"ysa uygulamanın ayar sayfası açılır. Kullanıcı
/// dönünce biter; arayüz durumu yeniden okur.
#[tauri::command]
pub async fn battery_settings(app: AppHandle) -> Res<()> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    call::<serde_json::Value>(app, "pilAyarlari", serde_json::json!({}))
        .await
        .map(|_| ())
        .map_err(|e| {
            tracing::warn!(error = %e, "pil ayarları açılamadı");
            AppError::new(
                ErrorCode::Internal,
                "Pil ayarları açılamadı. Telefonun Ayarlar › Uygulamalar › Mailhere › Pil bölümünden \"Kısıtlamasız\"ı seçebilirsin.",
            )
        })
}

/// Anlık bildirim (sürekli bağlantı) açık mı.
#[tauri::command]
pub async fn background_mode_get(app: AppHandle) -> Res<bool> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    call::<ModeResponse>(app, "arkaPlanDurumu", serde_json::json!({}))
        .await
        .map(|r| r.acik)
        .map_err(|e| {
            tracing::warn!(error = %e, "anlık bildirim durumu okunamadı");
            AppError::new(ErrorCode::Internal, "Anlık bildirim durumu okunamadı.")
        })
}

/// Anlık bildirimi açar ya da kapatır; son durumu döner. Açılış ön plan
/// hizmetini hemen başlatır (uygulama ekrandayken; Android 12+ arka plandan
/// başlatmaya izin vermez), sonraki açılışlarda da kendiliğinden başlar.
#[tauri::command]
pub async fn background_mode_set(app: AppHandle, on: bool) -> Res<bool> {
    if !cfg!(target_os = "android") {
        return Err(unsupported());
    }
    call::<ModeResponse>(app, "arkaPlanAyarla", serde_json::json!({ "acik": on }))
        .await
        .map(|r| r.acik)
        .map_err(|e| {
            tracing::warn!(error = %e, "anlık bildirim ayarlanamadı");
            AppError::new(
                ErrorCode::Internal,
                "Anlık bildirim başlatılamadı. Mailhere ekrandayken yeniden dene; sürerse pil kısıtlamasını kaldır.",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pil_durumu_kotlin_bicimi() {
        for (s, b) in [
            ("unrestricted", Battery::Unrestricted),
            ("optimized", Battery::Optimized),
            ("restricted", Battery::Restricted),
        ] {
            let r: BatteryResponse =
                serde_json::from_value(serde_json::json!({ "durum": s })).unwrap();
            assert_eq!(r.durum, b);
            assert_eq!(serde_json::to_value(b).unwrap(), s);
        }
        assert!(
            serde_json::from_value::<BatteryResponse>(serde_json::json!({ "durum": "x" })).is_err()
        );
    }

    #[test]
    fn masaustunde_desteklenmez() {
        let e = unsupported();
        assert_eq!(e.code, ErrorCode::Invalid);
        assert!(e.message.contains("Android"));
    }
}
