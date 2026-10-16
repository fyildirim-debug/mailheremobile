//! Mailhere Android kabuğu: tek pencere, Android anahtar kasası köprüsü,
//! sistem bildirimi (gruplu, eylemli; bildirimden Yanıtla / Sil / İstenmeyen
//! ya da kullanıcının seçtiği düğmeler), arka planda süren çekirdek (anlık
//! bildirim: pencere kapansa da süreç yaşar; etkinliksiz çekirdek: telefon
//! yeniden başlayınca hizmet pencere açılmadan eşitler ve bildirir), bildirim
//! sağlığı (pil kısıtlaması), açılışta başlama ayarı, dokunsal geri
//! bildirim, dosya seçme / kaydetme / paylaşma, QR tarayıcı (bilgisayardan
//! aktarım). Komutlar ve çekirdek ortak katmanda (`mh-app`, alt modül
//! `mailhere/`).
//!
//! Gradle `libmailhere_lib.so`yu yükler; etkinlik açılınca [`run`]
//! `tauri::mobile_entry_point` ile başlar, etkinlik yokken Kotlin `ArkaKopru`
//! JNI girişleriyle (`kopru.rs`) etkinliksiz çekirdeği (`cekirdek.rs`)
//! kullanır. Süreç başına tek çekirdek vardır. Aynı kabuk masaüstünde
//! geliştirme için ikili olarak da açılır (`src/main.rs`); orada anahtar
//! kasası yoktur, parolalar bellekte tutulur.

pub mod acilis;
pub mod arka;
pub mod bars;
pub mod cekirdek;
pub mod eylem;
pub mod files;
pub mod haptic;
pub mod keystore;
pub mod kopru;
pub mod notify;
pub mod platform;
pub mod plugin;
pub mod qr;

use std::sync::{Arc, OnceLock};

use mh_app::routes::is_app_url;
use mh_app::state::memory_mode;
use mh_app::{Dirs, Platform};
use mh_sync::{MemorySecrets, SecretStore};
use tauri::webview::PageLoadEvent;
use tauri::{App, AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::platform::AndroidPlatform;

/// Tek pencerenin etiketi (`capabilities/main.json` ile aynı).
pub const MAIN: &str = "main";

/// Süreçteki tek eşzamansız çalışma ortamı. Tauri'nin ortamı buna bir kez
/// bağlanır (`tauri::async_runtime::set` ikinci çağrıda panikler); bu yüzden
/// etkinliksiz çekirdek de, sonra açılan Tauri de aynı ortamı kullanır.
/// Kurulamazsa `None` (günlükte).
pub fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            // Tüm TLS istemcileri ring kullanır; varsayılan sağlayıcı sabitlenir.
            mh_app::logging::install_tls_provider();
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("mailhere")
                .build()
            {
                Ok(rt) => {
                    tauri::async_runtime::set(rt.handle().clone());
                    Some(rt)
                }
                Err(e) => {
                    tracing::error!(error = %e, "eşzamansız çalışma ortamı kurulamadı");
                    None
                }
            }
        })
        .as_ref()
}

/// Uygulamanın özel klasörleri. Android'de `app_local_data_dir` uygulamanın
/// veri kökü (`/data/user/0/<paket>`)dir; veritabanı onun `files` alt
/// klasörüne (standart `filesDir`) yazılır ki kök klasörün izinlerine
/// dokunulmasın. Önbellek `cacheDir`dir; açılan ekler oraya yazılır ve
/// `FileProvider` yalnız o klasörün `ekler/` altını paylaşır. Etkinliksiz
/// çekirdek aynı yolları Kotlin'den alır (`kopru::dirs`).
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
/// tarayıcıya gider). Sayfa yüklenince bildirimden açılması beklenen posta
/// açılır (`eylem::page_loaded`).
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
        .on_page_load(|w, p| {
            if p.event() == PageLoadEvent::Finished {
                eylem::page_loaded(w.app_handle());
            }
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
        Arc::new(keystore::AndroidKeystoreSecrets::new()?)
    };
    let dirs = Dirs::resolve(memory, || os_dirs(&handle))?;
    let platform = Arc::new(AndroidPlatform);
    let boot = mh_app::lock::Boot {
        rt,
        dirs,
        secrets,
        platform: platform.clone(),
        kdf: mh_app::lock::KdfParams::MOBILE,
    };
    // Parola kilidi açıksa çekirdek parola girilene kadar açılmaz: pencere
    // kilit ekranını gösterir, `lock_unlock` çekirdeği başlatır. Çekirdek
    // kapısı (etkinliksiz çekirdeği durdurma, hazır bildirimi, bekleyen
    // eylemler) `AndroidPlatform::core_starting/core_ready` ile iki yolda da
    // aynı sırayla çalışır.
    match mh_app::lock::prepare(&handle, boot) {
        Ok(Some(boot)) => {
            // Hata iletişim kutusu gösterildi; pencere açılmaz, kutu kapanınca çıkılır.
            if mh_app::setup_core(&handle, boot, None).is_err() {
                return Ok(());
            }
        }
        Ok(None) => tracing::info!("parola kilidi: çekirdek parola bekliyor"),
        Err(e) => {
            platform.fatal(&handle, &e);
            return Ok(());
        }
    }
    create_main(&handle)?;
    Ok(())
}

/// Günlük katmanı: Android'de logcat (etiket `Mailhere`), masaüstünde konsol.
/// İkinci çağrı sessizce yok sayılır; hiçbir yol paniklemez.
pub fn init_logging() {
    #[cfg(target_os = "android")]
    {
        match tracing_android::layer("Mailhere") {
            Ok(layer) => mh_app::logging::init(layer),
            Err(_) => mh_app::logging::init(tracing_subscriber::fmt::layer().with_target(false)),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        mh_app::logging::init(tracing_subscriber::fmt::layer().with_target(false));
    }
}

/// "Anlık bildirim" açık mı (Kotlin tercihi; seçilmemişse açık). Köprü yoksa
/// (masaüstü) kapalı sayılır: pencere kapanınca süreç biter.
fn arka_plan_acik() -> bool {
    crate::kopru::anlik_acik().unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();
    let Some(rt) = runtime() else {
        tracing::error!("Mailhere başlatılamadı: çalışma ortamı yok");
        return;
    };
    let rt = rt.handle().clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(plugin::init())
        .setup(move |app| setup(app, rt.clone()))
        // Ortak komutlar + yalnız Android komutları (`commands.txt`);
        // kilitliyken `lock_*` dışındakiler kapıda reddedilir.
        .invoke_handler(mh_app::lock::gate(mh_app::shared_commands![
            files::attachment_share,
            files::attachment_save,
            notify::notification_permission,
            notify::notification_settings,
            notify::notification_status,
            qr::qr_scan,
            bars::system_bars,
            arka::battery_status,
            arka::battery_settings,
            arka::background_mode_get,
            arka::background_mode_set,
            haptic::haptic,
            acilis::autostart_get,
            acilis::autostart_set,
        ]))
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            tracing::error!(error = %e, "Mailhere başlatılamadı");
            return;
        }
    };
    app.run(|_app, event| {
        // Son pencere (etkinlik) yok edildi: anlık bildirim açıkken süreç ve
        // çekirdek yaşar — IMAP IDLE / JMAP akışı sürer, yeni posta bildirilir,
        // bildirim düğmeleri çalışır (ön plan hizmeti süreci canlı tutar).
        // Kapalıyken eskisi gibi çıkılır. Kullanıcı uygulamayı yeniden
        // açınca Kotlin süreci temiz başlatır (`YenidenBaslat`): Tauri aynı
        // süreçte ikinci bir pencere kuramaz.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = &event
        {
            if arka_plan_acik() {
                api.prevent_exit();
                tracing::info!(
                    "pencere kapandı; anlık bildirim açık, çekirdek arka planda sürüyor"
                );
            }
        }
    });
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

    /// İki kabukta aynı adla, kabuğa özgü gövdeyle tanımlı komutlar (ortak
    /// listede değil; arayüz aynı adı çağırır).
    const BOTH_SHELLS: &[&str] = &["autostart_get", "autostart_set"];

    /// Masaüstü kabuğunun komutları; Android yetenek dosyasında bunlara izin
    /// yoktur (iki kabukta aynı adlı olanlar hariç).
    fn desktop_commands() -> Vec<&'static str> {
        names(include_str!("../../mailhere/src-tauri/commands.txt"))
    }

    fn desktop_only() -> Vec<&'static str> {
        desktop_commands()
            .into_iter()
            .filter(|n| !BOTH_SHELLS.contains(n))
            .collect()
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
                !desktop_only().contains(name),
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
        for name in desktop_only() {
            assert!(
                !allowed.contains(name),
                "allow-{name}: masaüstüne özel komut Android yetenek dosyasında"
            );
        }
    }

    #[test]
    fn iki_kabukta_ayni_adli_komutlar_android_listesinde() {
        for name in BOTH_SHELLS {
            assert!(
                mobile_commands().contains(name),
                "{name}: Android commands.txt'de yok"
            );
            assert!(
                !mh_app::SHARED_COMMANDS.contains(name),
                "{name}: ortak listede; kabuğa özgü olmalı"
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

    #[test]
    fn calisma_ortami_sureçte_tektir() {
        let a = super::runtime().expect("çalışma ortamı");
        let b = super::runtime().expect("çalışma ortamı");
        assert!(std::ptr::eq(a, b));
    }
}
