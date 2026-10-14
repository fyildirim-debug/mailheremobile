//! Sistem çubukları: durum ve gezinme çubuğu simgeleri uygulamanın temasını
//! izler. Pencere kenardan kenaradır (MainActivity.enableEdgeToEdge); tema
//! dosyası yalnız telefonun gece kipini bilir, kullanıcı uygulamada başka
//! tema seçmiş olabilir.

use mh_core::AppError;
use tauri::AppHandle;

type Res<T> = Result<T, AppError>;

/// Koyu temada açık renk simgeler, açık temada koyu simgeler.
#[tauri::command]
pub async fn system_bars(app: AppHandle, dark: bool) -> Res<()> {
    if !cfg!(target_os = "android") {
        return Ok(());
    }
    // Eklenti çağrısı ana iş parçacığını beklemesin (`qr_scan` ile aynı).
    let res = tokio::task::spawn_blocking(move || {
        crate::plugin::call::<serde_json::Value>(
            &app,
            "systemBars",
            serde_json::json!({ "dark": dark }),
        )
    })
    .await
    .map_err(mh_app::state::internal)?;
    // Yalnız simge rengidir; uygulanamazsa çalışma sürer, günlüğe yazılır.
    if let Err(e) = res {
        tracing::warn!(error = %e, "sistem çubukları ayarlanamadı");
    }
    Ok(())
}
