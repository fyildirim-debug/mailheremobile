//! Etkinliksiz JNI köprüsü: Rust ↔ Kotlin `ArkaKopru` (eklentisiz).
//!
//! Tauri'nin eklenti köprüsü (`run_mobile_plugin`) etkinlik ister; etkinlik
//! yok edilmişken çağrılırsa `first_activity_id().expect(…)` ile panikler
//! (sürüm profili `panic = "abort"`: süreç çöker). Çekirdeğin kendi
//! başlattığı her iş — kasa okuma/yazma (her IMAP bağlantısı), yeni posta
//! bildirimi, bildirimleri uzlaştırma, bildirim eylemlerinin sonucu — bu
//! yüzden etkinlik olsa da olmasa da buradan geçer. Yalnız arayüzün
//! çağırdığı komutlar (dosya seç/aç/paylaş, QR, çubuklar, dokunsal geri
//! bildirim, pil ve anlık bildirim ayarı, bildirim izni) eklentide kalır;
//! onlar ancak pencere varken çağrılabilir.
//!
//! Köprü `Application.onCreate` içinde `ArkaKopru.kur` ile kurulur: `JavaVM`,
//! uygulama `Context`inin ve `ArkaKopru` sınıfının `GlobalRef`i saklanır.
//! Sınıf, çağıran iş parçacığı ne olursa olsun bu `GlobalRef` üzerinden
//! bulunur (Rust iş parçacıklarının sınıf yükleyicisi uygulama sınıflarını
//! görmez; `FindClass` oralarda başarısız olur). Her çağrı kendi yerel
//! çerçevesinde çalışır (`with_local_frame`): kalıcı bağlanmış iş
//! parçacıklarında yerel başvurular birikmez. Hiçbir yol paniklemez; hata
//! `Err(String)` olarak döner ve yalnız günlüğe yazılır.
//!
//! Kotlin'den çağrılan giriş noktaları (`Java_…_ArkaKopru_*`) en altta;
//! Kotlin tarafındaki `external fun` imzalarıyla birebir eşleşir (birim
//! testi eşler).

use mh_app::Dirs;
use mh_core::MessageId;

/// Kotlin `ArkaKopru` sınıfının statik yöntemleri ve JNI imzaları
/// (`Context` ilk parametredir). Test, adların Kotlin dosyasında
/// `@JvmStatic fun` olarak bulunduğunu denetler.
pub const KOTLIN_YONTEMLER: &[(&str, &str)] = &[
    (
        "kasaOku",
        "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
    ),
    (
        "kasaYaz",
        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
    ),
    ("kasaSil", "(Landroid/content/Context;Ljava/lang/String;)V"),
    ("bildir", "(Landroid/content/Context;Ljava/lang/String;)V"),
    ("kaldir", "(Landroid/content/Context;Ljava/lang/String;)V"),
    ("hata", "(Landroid/content/Context;JLjava/lang/String;)V"),
    (
        "yanitKuyrukta",
        "(Landroid/content/Context;JLjava/lang/String;JLjava/lang/String;)V",
    ),
    ("yanitGeriAlindi", "(Landroid/content/Context;JZ)V"),
    ("etkin", "(Landroid/content/Context;)Ljava/lang/String;"),
    (
        "bildirimlerAcik",
        "(Landroid/content/Context;Ljava/lang/String;)Z",
    ),
    (
        "bekleyenler",
        "(Landroid/content/Context;)Ljava/lang/String;",
    ),
    ("anlikAcik", "(Landroid/content/Context;)Z"),
];

/// Rust'ın dışa verdiği JNI giriş noktaları (Kotlin `external fun` adları).
pub const JNI_GIRISLER: &[&str] = &[
    "kur",
    "cekirdekBaslat",
    "cekirdekDurdur",
    "cekirdekDurumu",
    "esitle",
    "eylem",
    "tekSefer",
];

/// Giriş noktalarının `.so` içindeki sembol öneki.
pub const SEMBOL_ONEK: &str = "Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_";

/// Kotlin yönteminin JNI imzası (bilinmiyorsa boş).
pub fn sig(name: &str) -> &'static str {
    KOTLIN_YONTEMLER
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
        .unwrap_or("")
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::OnceLock;

    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue, JValueOwned};
    use jni::sys::{jboolean, jint, jlong, JNI_FALSE, JNI_TRUE};
    use jni::{JNIEnv, JavaVM};
    use mh_app::Dirs;

    use super::sig;

    pub(super) struct Kopru {
        vm: JavaVM,
        ctx: GlobalRef,
        sinif: GlobalRef,
        pub(super) dirs: Dirs,
    }

    pub(super) static KOPRU: OnceLock<Kopru> = OnceLock::new();

    /// Köprü hatası; `with_local_frame` için `From<jni::errors::Error>`.
    pub(super) struct Hata(pub String);

    impl From<jni::errors::Error> for Hata {
        fn from(e: jni::errors::Error) -> Self {
            Hata(e.to_string())
        }
    }

    /// Bekleyen Java istisnasını günlüğe yazıp temizler (istisna dururken
    /// başka JNI çağrısı yapılamaz).
    fn istisna_temizle(env: &mut JNIEnv) {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
    }

    /// `ArkaKopru`nun statik yöntemini çağırır; istisna hataya döner.
    pub(super) fn statik<'l>(
        env: &mut JNIEnv<'l>,
        sinif: &GlobalRef,
        ad: &str,
        args: &[JValue],
    ) -> Result<JValueOwned<'l>, Hata> {
        match env.call_static_method(sinif, ad, sig(ad), args) {
            Ok(v) => Ok(v),
            Err(e) => {
                istisna_temizle(env);
                Err(Hata(format!("{ad}: {e}")))
            }
        }
    }

    /// Java `String` (ya da `null`) → Rust.
    pub(super) fn metin(env: &mut JNIEnv, v: JValueOwned) -> Result<Option<String>, Hata> {
        let obj = v.l()?;
        if obj.is_null() {
            return Ok(None);
        }
        let js = JString::from(obj);
        let s = env.get_string(&js)?;
        Ok(Some(String::from(s)))
    }

    /// Köprü kuruluysa çağıran iş parçacığını JVM'e bağlar ve `f`yi kendi
    /// yerel çerçevesinde çalıştırır.
    pub(super) fn with_env<T>(
        f: impl FnOnce(&mut JNIEnv, &JObject, &GlobalRef) -> Result<T, Hata>,
    ) -> Result<T, String> {
        let Some(k) = KOPRU.get() else {
            return Err("JNI köprüsü kurulmadı".to_owned());
        };
        let mut env =
            k.vm.attach_current_thread_permanently()
                .map_err(|e| format!("iş parçacığı JVM'e bağlanamadı: {e}"))?;
        env.with_local_frame(32, |env| f(env, k.ctx.as_obj(), &k.sinif))
            .map_err(|e: Hata| e.0)
    }

    pub(super) fn jstr<'l>(env: &mut JNIEnv<'l>, s: &str) -> Result<JString<'l>, Hata> {
        Ok(env.new_string(s)?)
    }

    fn string_arg(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(String::from).unwrap_or_default()
    }

    // ------------------------------------------------------------ girişler

    /// `ArkaKopru.kur(ctx, veri, onbellek)`: `Application.onCreate` içinde,
    /// süreçte bir kez. Günlük ve çalışma ortamı da burada kurulur ki daha
    /// sonra açılan Tauri aynı ortamı kullansın (`tauri::async_runtime::set`
    /// süreçte yalnız bir kez çağrılabilir).
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_kur(
        mut env: JNIEnv,
        sinif: JClass,
        ctx: JObject,
        veri: JString,
        onbellek: JString,
    ) {
        crate::init_logging();
        if KOPRU.get().is_some() {
            return;
        }
        let (Ok(vm), Ok(ctx), Ok(sinif)) = (
            env.get_java_vm(),
            env.new_global_ref(&ctx),
            env.new_global_ref(&sinif),
        ) else {
            tracing::error!("JNI köprüsü kurulamadı (JavaVM ya da GlobalRef)");
            return;
        };
        let veri = string_arg(&mut env, &veri);
        let onbellek = string_arg(&mut env, &onbellek);
        if veri.is_empty() {
            tracing::error!("JNI köprüsü: veri klasörü boş");
            return;
        }
        let dirs = Dirs {
            data: veri.into(),
            cache: onbellek.into(),
            legacy_db: None,
        };
        let _ = KOPRU.set(Kopru {
            vm,
            ctx,
            sinif,
            dirs,
        });
        // Çalışma ortamı erkenden: Tauri'nin eşzamansız ortamı tek kez atanır.
        if crate::runtime().is_none() {
            tracing::error!("eşzamansız çalışma ortamı kurulamadı");
        }
        tracing::info!("JNI köprüsü kuruldu");
    }

    /// Etkinliksiz çekirdeği başlatır; sonuç kodu `crate::cekirdek::Baslatma`.
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_cekirdekBaslat(
        _env: JNIEnv,
        _sinif: JClass,
    ) -> jint {
        crate::cekirdek::basliksiz_baslat().kod()
    }

    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_cekirdekDurdur(
        _env: JNIEnv,
        _sinif: JClass,
    ) {
        crate::cekirdek::basliksiz_durdur();
    }

    /// 0 yok, 1 etkinliksiz çekirdek, 2 Tauri çekirdeği (bkz. `Durum::kod`).
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_cekirdekDurumu(
        _env: JNIEnv,
        _sinif: JClass,
    ) -> jint {
        crate::cekirdek::durum().kod()
    }

    /// Çalışan çekirdeğe "şimdi eşitle" (uyanma alarmı, yoklama işi).
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_esitle(
        _env: JNIEnv,
        _sinif: JClass,
    ) -> jboolean {
        if crate::cekirdek::sync_now() {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }

    /// Bildirim eylemi (JSON, `eylem.rs` biçimi). Çalışan çekirdek yoksa
    /// `false`: Kotlin eylemi kasaya yazar, çekirdek açılınca uygulanır.
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_eylem(
        mut env: JNIEnv,
        _sinif: JClass,
        json: JString,
    ) -> jboolean {
        let json = string_arg(&mut env, &json);
        let e = match serde_json::from_str::<crate::eylem::Eylem>(&json) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "bildirim eylemi okunamadı");
                return JNI_FALSE;
            }
        };
        if crate::eylem::dispatch_global(e) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }

    /// Tek seferlik eşitle-bildir (JobScheduler yedeği); en çok `sure` ms.
    #[no_mangle]
    pub extern "system" fn Java_com_furkanyildirim_mailhere_plugin_ArkaKopru_tekSefer(
        _env: JNIEnv,
        _sinif: JClass,
        sure: jlong,
    ) -> jint {
        let sure = std::time::Duration::from_millis(sure.clamp(1_000, 600_000) as u64);
        crate::cekirdek::tek_sefer(sure).kod()
    }
}

/// Köprü kurulu mu (`Application.onCreate` çalıştı mı)?
pub fn kuruldu() -> bool {
    #[cfg(target_os = "android")]
    {
        android::KOPRU.get().is_some()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}

/// Kotlin'in verdiği veri ve önbellek klasörleri (Tauri'nin
/// `app_local_data_dir`/`files` ve `app_cache_dir` ile aynı yollar).
pub fn dirs() -> Option<Dirs> {
    #[cfg(target_os = "android")]
    {
        android::KOPRU.get().map(|k| Dirs {
            data: k.dirs.data.clone(),
            cache: k.dirs.cache.clone(),
            legacy_db: None,
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

#[cfg(not(target_os = "android"))]
fn yalniz_android() -> String {
    "yalnız Android'de".to_owned()
}

// ------------------------------------------------------------------ kasa

/// `SecretVault.get`: kayıt yoksa `None`; çözülemiyorsa hata.
pub fn kasa_oku(key: &str) -> Result<Option<String>, String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let k = android::jstr(env, key)?;
            let v = android::statik(
                env,
                sinif,
                "kasaOku",
                &[JValue::Object(ctx), JValue::Object(&k)],
            )?;
            android::metin(env, v)
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = key;
        Err(yalniz_android())
    }
}

pub fn kasa_yaz(key: &str, value: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let k = android::jstr(env, key)?;
            let v = android::jstr(env, value)?;
            android::statik(
                env,
                sinif,
                "kasaYaz",
                &[JValue::Object(ctx), JValue::Object(&k), JValue::Object(&v)],
            )?;
            Ok(())
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (key, value);
        Err(yalniz_android())
    }
}

pub fn kasa_sil(key: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let k = android::jstr(env, key)?;
            android::statik(
                env,
                sinif,
                "kasaSil",
                &[JValue::Object(ctx), JValue::Object(&k)],
            )?;
            Ok(())
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = key;
        Err(yalniz_android())
    }
}

// -------------------------------------------------------------- bildirim

/// Context + tek JSON metni alan, değer dönmeyen yöntemler.
#[cfg(target_os = "android")]
fn metinli(ad: &str, json: &str) -> Result<(), String> {
    android::with_env(|env, ctx, sinif| {
        let j = android::jstr(env, json)?;
        android::statik(env, sinif, ad, &[JValue::Object(ctx), JValue::Object(&j)])?;
        Ok(())
    })
}

/// Yeni posta bildirimleri: `{ gruplar: HesapGrubu[] }` (`notify.rs`).
pub fn bildir(gruplar: &serde_json::Value) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        metinli("bildir", &gruplar.to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = gruplar;
        Err(yalniz_android())
    }
}

/// Postaların bildirimlerini kaldırır; özetler güncellenir.
pub fn bildirim_kaldir(ids: &[MessageId]) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        metinli("kaldir", &serde_json::json!(ids).to_string())
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = ids;
        Err(yalniz_android())
    }
}

/// Eylem uygulanamadı: bildirimde neden ve çözüm.
pub fn bildirim_hata(id: MessageId, metin: &str) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let m = android::jstr(env, metin)?;
            android::statik(
                env,
                sinif,
                "hata",
                &[JValue::Object(ctx), JValue::Long(id), JValue::Object(&m)],
            )?;
            Ok(())
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, metin);
        Err(yalniz_android())
    }
}

/// Yanıt giden kutusunda: geri sayım ve "Geri al".
pub fn yanit_kuyrukta(
    id: MessageId,
    kuyruk: &str,
    gonderim: i64,
    metin: &str,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let k = android::jstr(env, kuyruk)?;
            let m = android::jstr(env, metin)?;
            android::statik(
                env,
                sinif,
                "yanitKuyrukta",
                &[
                    JValue::Object(ctx),
                    JValue::Long(id),
                    JValue::Object(&k),
                    JValue::Long(gonderim),
                    JValue::Object(&m),
                ],
            )?;
            Ok(())
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, kuyruk, gonderim, metin);
        Err(yalniz_android())
    }
}

pub fn yanit_geri_alindi(id: MessageId, iptal: bool) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            android::statik(
                env,
                sinif,
                "yanitGeriAlindi",
                &[
                    JValue::Object(ctx),
                    JValue::Long(id),
                    JValue::Bool(u8::from(iptal)),
                ],
            )?;
            Ok(())
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (id, iptal);
        Err(yalniz_android())
    }
}

/// Context alıp JSON metni dönen yöntemler.
#[cfg(target_os = "android")]
fn json_don<T: serde::de::DeserializeOwned>(ad: &str) -> Result<T, String> {
    let s = android::with_env(|env, ctx, sinif| {
        let v = android::statik(env, sinif, ad, &[JValue::Object(ctx)])?;
        android::metin(env, v)
    })?
    .unwrap_or_else(|| "[]".to_owned());
    serde_json::from_str(&s).map_err(|e| format!("{ad}: {e}"))
}

/// Hâlâ "yeni posta" durumundaki bildirimlerin posta kimlikleri.
pub fn etkin_bildirimler() -> Result<Vec<MessageId>, String> {
    #[cfg(target_os = "android")]
    {
        json_don("etkin")
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(yalniz_android())
    }
}

/// Süreç kapalıyken kasaya yazılan eylemler ve soğuk açılıştaki dokunuş;
/// alınınca silinir.
pub fn bekleyen_eylemler() -> Result<Vec<serde_json::Value>, String> {
    #[cfg(target_os = "android")]
    {
        json_don("bekleyenler")
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(yalniz_android())
    }
}

/// Uygulamanın ve verilen kanalın bildirimleri açık mı.
pub fn bildirimler_acik(kanal: &str) -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let k = android::jstr(env, kanal)?;
            let v = android::statik(
                env,
                sinif,
                "bildirimlerAcik",
                &[JValue::Object(ctx), JValue::Object(&k)],
            )?;
            Ok(v.z()?)
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = kanal;
        Err(yalniz_android())
    }
}

/// "Anlık bildirim" açık mı (Kotlin tercihi; seçilmemişse açık).
pub fn anlik_acik() -> Result<bool, String> {
    #[cfg(target_os = "android")]
    {
        android::with_env(|env, ctx, sinif| {
            let v = android::statik(env, sinif, "anlikAcik", &[JValue::Object(ctx)])?;
            Ok(v.z()?)
        })
    }
    #[cfg(not(target_os = "android"))]
    {
        Err(yalniz_android())
    }
}

#[cfg(target_os = "android")]
use jni::objects::JValue;

#[cfg(test)]
mod tests {
    use super::*;

    const KOTLIN: &str = include_str!(
        "../gen/android/app/src/main/java/com/furkanyildirim/mailhere/plugin/ArkaKopru.kt"
    );

    /// Rust'ın adıyla çağırdığı her Kotlin yöntemi `ArkaKopru`da `@JvmStatic
    /// fun` olarak var; her imza `Context` ile başlar.
    #[test]
    fn kotlin_yontemleri_var_ve_imzalar_context_ile_baslar() {
        for (ad, imza) in KOTLIN_YONTEMLER {
            assert!(
                KOTLIN.contains(&format!("@JvmStatic fun {ad}(")),
                "{ad}: ArkaKopru.kt'de @JvmStatic fun yok"
            );
            assert!(
                imza.starts_with("(Landroid/content/Context;"),
                "{ad}: imza Context ile başlamalı"
            );
            assert_eq!(sig(ad), *imza);
        }
        assert_eq!(sig("yok"), "");
    }

    /// Rust'ın dışa verdiği her giriş noktası Kotlin'de `@JvmStatic external
    /// fun`; Kotlin'in her `external fun`ı Rust'ta var (sembol adı eşlemesi
    /// `.so` üzerinde `scripts/apk-denetle.sh` ile de denetlenir).
    #[test]
    fn jni_girisleri_kotlin_external_ile_birebir() {
        let rust_src = include_str!("kopru.rs");
        for ad in JNI_GIRISLER {
            assert!(
                KOTLIN.contains(&format!("@JvmStatic external fun {ad}(")),
                "{ad}: ArkaKopru.kt'de external fun yok"
            );
            assert!(
                rust_src.contains(&format!("fn {SEMBOL_ONEK}{ad}(")),
                "{ad}: Rust'ta {SEMBOL_ONEK}{ad} yok"
            );
        }
        let externals = KOTLIN
            .lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix("@JvmStatic external fun ")
                    .and_then(|r| r.split('(').next())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            externals.len(),
            JNI_GIRISLER.len(),
            "Kotlin'de fazla external fun var"
        );
        for e in externals {
            assert!(JNI_GIRISLER.contains(&e), "{e}: Rust girişi yok");
        }
    }

    #[test]
    fn masaustunde_kopru_yok() {
        assert!(!kuruldu());
        assert!(dirs().is_none());
        assert!(kasa_oku("x").is_err());
        assert!(bildir(&serde_json::json!({})).is_err());
        assert!(anlik_acik().is_err());
    }
}
