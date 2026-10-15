//! Dokunsal geri bildirim (yalnız Android komutu): kaydırma eşiği, onay ve
//! ret. Kotlin `View.performHapticFeedback` kullanır; sistemin "dokunma
//! geri bildirimi" ayarına uyar (kapalıysa titreşmez). API 34+
//! `GESTURE_THRESHOLD_ACTIVATE`, API 30+ `CONFIRM` / `REJECT`; eski
//! sürümlerde `CLOCK_TICK` / `VIRTUAL_KEY` / `LONG_PRESS`.

use mh_core::AppError;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

type Res<T> = Result<T, AppError>;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HapticKind {
    /// Kaydırma eşiği geçildi.
    Tick,
    /// Eylem tamamlandı.
    Confirm,
    /// Eylem reddedildi / geri alındı.
    Reject,
}

/// `haptic { kind }` → null. Android dışında sessizce geçer; titreşim
/// yapılamazsa da hata dönmez (yalnız geri bildirimdir).
#[tauri::command]
pub async fn haptic(app: AppHandle, kind: HapticKind) -> Res<()> {
    if !cfg!(target_os = "android") {
        return Ok(());
    }
    let res = tokio::task::spawn_blocking(move || {
        crate::plugin::call::<serde_json::Value>(
            &app,
            "haptic",
            serde_json::json!({ "kind": kind }),
        )
    })
    .await
    .map_err(mh_app::state::internal)?;
    if let Err(e) = res {
        tracing::debug!(error = %e, "dokunsal geri bildirim verilemedi");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tur_adlari() {
        for (s, k) in [
            ("tick", HapticKind::Tick),
            ("confirm", HapticKind::Confirm),
            ("reject", HapticKind::Reject),
        ] {
            assert_eq!(
                serde_json::from_value::<HapticKind>(serde_json::json!(s)).unwrap(),
                k
            );
            assert_eq!(serde_json::to_value(k).unwrap(), s);
        }
        assert!(serde_json::from_value::<HapticKind>(serde_json::json!("buzz")).is_err());
    }
}
