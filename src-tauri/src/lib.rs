//! Mailhere Android kabuğu: tek pencere, Android anahtar kasası köprüsü,
//! sistem bildirimi, dosya seçme / kaydetme / paylaşma, QR tarayıcı
//! (bilgisayardan aktarım). Komutlar ve çekirdek ortak katmanda (`mh-app`,
//! alt modül `mailhere/`).
//!
//! Gradle `libmailhere_lib.so`yu yükler ve [`run`] `tauri::mobile_entry_point`
//! ile başlar. Aynı kabuk masaüstünde geliştirme için ikili olarak da açılır
//! (`src/main.rs`); orada anahtar kasası yoktur, parolalar bellekte tutulur.

pub mod bars;
pub mod files;
pub mod keystore;
pub mod notify;
pub mod platform;
pub mod plugin;
pub mod qr;

use std::sync::Arc;

use mh_app::routes::is_app_url;
use mh_app::state::memory_mode;
use mh_app::Dirs;
use mh_sync::{MemorySecrets, SecretStore};
use tauri::{App, AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::platform::AndroidPlatform;

/// Tek pencerenin etiketi (`capabilities/main.json` ile aynı).
pub const MAIN: &str = "main";

/// Uygulamanın özel klasörleri. Android'de `app_local_data_dir` uygulamanın
/// veri kökü (`/data/user/0/<paket>`)dir; veritabanı onun `files` alt
/// klasörüne (standart `filesDir`) yazılır ki kök klasörün izinlerine
/// dokunulmasın. Önbellek `cacheDir`dir; açılan ekler oraya yazılır ve
/// `FileProvider` yalnız o klasörün `ekler/` altını paylaşır.
fn os_dirs(app: &AppHandle) -> Result<Dirs, Box<dyn std::error::Error>> {
    let root = app.path().app_local_data_dir()?;
    Ok(Dirs {
        cache: app
            .path()
            .app_cache_dir()
            .unwrap_or_else(|_| root.join("cache")),
        data: root.join("files"),
        legacy_db: None,
    })
}

/// Geliştirme sunucusunun ana makinesi (`tauri.conf.json` → `devUrl`);
/// `is_app_url` yalnız geliştirme derlemesinde ona izin verir.
fn dev_host(app: &AppHandle) -> Option<String> {
    app.config()
        .build
        .dev_url
        .as_ref()
        .and_then(|u| u.host_str())
        .map(str::to_owned)
}

/// Ana pencere: tek sayfa, yönlendirme bekçili (posta içindeki bağlantı ya da
/// bir betik pencereyi uygulama dışı adrese götüremez; dış bağlantılar
/// tarayıcıya gider).
pub fn create_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(w) = app.get_webview_window(MAIN) {
        return Ok(w);
    }
    let dev = dev_host(app);
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html#/main".into()))
        .on_navigation(move |url| {
            let ok = is_app_url(url, dev.as_deref());
            if !ok {
                tracing::warn!(scheme = url.scheme(), host = ?url.host_str(), "uygulama dışı adrese yönlendirme engellendi");
            }
            ok
        })
        .title("Mailhere")
        .build()
}

fn setup(app: &mut App, rt: tokio::runtime::Handle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    // Tanıtım ve deneme kipi: parolalar ve veritabanı anahtarı bellekte.
    let memory = memory_mode();
    let secrets: Arc<dyn SecretStore> = if memory {
        tracing::info!("parolalar ve veritabanı anahtarı bellekte (tanıtım ya da test kipi)");
        Arc::new(MemorySecrets::new())
    } else {
        Arc::new(keystore::AndroidKeystoreSecrets::new(&handle)?)
    };
    let dirs = Dirs::resolve(memory, || os_dirs(&handle))?;
    let platform = Arc::new(AndroidPlatform);
    // Hata iletişim kutusu gösterildi; pencere açılmaz, kutu kapanınca çıkılır.
    if mh_app::setup_core(app, rt, dirs, secrets, platform).is_err() {
        return Ok(());
    }
    notify::setup(&handle);
    create_main(&handle)?;
    Ok(())
}

/// Günlük katmanı: Android'de logcat (etiket `Mailhere`), masaüstünde konsol.
#[cfg(target_os = "android")]
fn log_layer(
) -> impl tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync + 'static {
    tracing_android::layer("Mailhere").expect("logcat katmanı kurulamadı")
}

#[cfg(not(target_os = "android"))]
fn log_layer(
) -> impl tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync + 'static {
    tracing_subscriber::fmt::layer().with_target(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    mh_app::logging::init(log_layer());
    let runtime = mh_app::logging::runtime();
    let rt = runtime.handle().clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(plugin::init())
        .setup(move |app| setup(app, rt.clone()))
        // Ortak komutlar + yalnız Android komutları (`commands.txt`).
        .invoke_handler(mh_app::shared_commands![
            files::attachment_share,
            files::attachment_save,
            notify::notification_permission,
            qr::qr_scan,
            bars::system_bars,
        ])
        .run(tauri::generate_context!())
        .expect("Mailhere başlatılamadı");
    drop(runtime);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    fn names(list: &str) -> Vec<&str> {
        list.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect()
    }

    /// Yalnız Android komutları (`commands.txt`).
    fn mobile_commands() -> Vec<&'static str> {
        names(include_str!("../commands.txt"))
    }

    /// Masaüstü kabuğunun komutları; Android yetenek dosyasında bunlara izin yoktur.
    fn desktop_commands() -> Vec<&'static str> {
        names(include_str!("../../mailhere/src-tauri/commands.txt"))
    }

    fn capability() -> serde_json::Value {
        serde_json::from_str(include_str!("../capabilities/main.json"))
            .expect("yetenek dosyası JSON")
    }

    /// `capabilities/main.json` içindeki uygulama komutu izinleri
    /// (`allow-<ad>`; eklenti izinleri `:` taşır ve sayılmaz).
    fn allowed() -> BTreeSet<String> {
        let cap = capability();
        let mut out = BTreeSet::new();
        for p in cap["permissions"].as_array().expect("permissions dizisi") {
            let p = p.as_str().expect("izin adı");
            if let Some(name) = p.strip_prefix("allow-").filter(|_| !p.contains(':')) {
                out.insert(name.replace('-', "_"));
            }
        }
        out
    }

    #[test]
    fn android_komutlari_ortaklarla_cakismaz_ve_bagli() {
        let mobile = mobile_commands();
        assert!(!mobile.is_empty());
        for name in &mobile {
            assert!(
                !mh_app::SHARED_COMMANDS.contains(name),
                "{name}: hem ortak hem Android listesinde"
            );
            assert!(
                !desktop_commands().contains(name),
                "{name}: masaüstü kabuğunun komutuyla aynı ad"
            );
            // Ad `shared_commands!` çağrısına `modül::ad,` biçiminde eklenmiş olmalı.
            assert!(
                include_str!("lib.rs").contains(&format!("::{name},")),
                "{name}: lib.rs'teki shared_commands! çağrısında yok"
            );
        }
    }

    #[test]
    fn her_komut_izinli_ve_her_izin_bir_komut() {
        let allowed = allowed();
        let known: BTreeSet<String> = mh_app::SHARED_COMMANDS
            .iter()
            .chain(mobile_commands().iter())
            .map(|s| s.to_string())
            .collect();
        for name in &known {
            assert!(
                allowed.contains(name),
                "{name}: capabilities/main.json'da allow-* yok"
            );
        }
        for name in &allowed {
            assert!(known.contains(name), "allow-{name}: böyle bir komut yok");
        }
    }

    #[test]
    fn masaustune_ozel_izin_yok() {
        let allowed = allowed();
        for name in desktop_commands() {
            assert!(
                !allowed.contains(name),
                "allow-{name}: masaüstüne özel komut Android yetenek dosyasında"
            );
        }
    }

    #[test]
    fn yetenek_yalniz_android_ve_tek_pencere() {
        let cap = capability();
        assert_eq!(cap["platforms"], serde_json::json!(["android"]));
        assert_eq!(cap["windows"], serde_json::json!([super::MAIN]));
        assert_eq!(cap["identifier"], "main");
    }
}
