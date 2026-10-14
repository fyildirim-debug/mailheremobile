//! Uygulama içi Android eklentisi (`MhPlugin.kt`): anahtar kasası, dosya adı,
//! dosya açma ve paylaşma. Rust tarafı Kotlin komutlarını
//! `PluginHandle::run_mobile_plugin` ile eşzamanlı çağırır; arayüzden
//! çağrılan komutu yoktur (izin dosyası gerekmez).

use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Wry};

/// Eklenti adı (Tauri eklenti kaydı).
pub const NAME: &str = "mh-android";
/// Kotlin sınıfının paketi ve adı (`gen/android/app/src/main/java/...`).
pub const IDENTIFIER: &str = "com.furkanyildirim.mailhere.plugin";
pub const CLASS: &str = "MhPlugin";

/// Kotlin eklentisinin tutamacı; Tauri durumunda yönetilir.
#[cfg(target_os = "android")]
pub struct MhAndroid(pub tauri::plugin::PluginHandle<Wry>);

/// Kabuk yalnız wry çalışma zamanıyla derlenir; eklenti de ona bağlıdır.
pub fn init() -> TauriPlugin<Wry> {
    Builder::<Wry>::new(NAME)
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = api.register_android_plugin(IDENTIFIER, CLASS)?;
                app.manage(MhAndroid(handle));
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = (app, api);
            }
            Ok(())
        })
        .build()
}

/// Kotlin komutunu çağırır. Android dışında eklenti yoktur; hata döner.
pub fn call<T: DeserializeOwned>(
    app: &AppHandle,
    command: &str,
    payload: impl Serialize,
) -> Result<T, String> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let plugin = app
            .try_state::<MhAndroid>()
            .ok_or_else(|| "Android eklentisi kayıtlı değil".to_owned())?;
        plugin
            .0
            .run_mobile_plugin::<T>(command, payload)
            .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, payload);
        Err(format!("{command}: yalnız Android'de"))
    }
}
