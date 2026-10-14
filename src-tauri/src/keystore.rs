//! Android anahtar kasası köprüsü: parolalar, OAuth yenileme jetonları ve
//! veritabanı anahtarı (`db-key`) `SecretVault.kt`te, `AndroidKeyStore` ile
//! korunan AES-256-GCM anahtarıyla şifrelenir ve `mailhere-kasa` tercih
//! dosyasında durur. Buradaki kod yalnız JSON eşlemesi yapar; değerler
//! günlüğe ya da hata metnine yazılmaz.
//!
//! Kotlin komutları: `secretGet { key } → { value: string | null }`,
//! `secretSet { key, value } → null`, `secretDelete { key } → null`.

use mh_core::{AppError, AppResult, ErrorCode};
use mh_sync::SecretStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

/// Kotlin tarafına giden çağrı; testte sahte uygulamayla değiştirilir.
pub trait VaultBridge: Send + Sync + 'static {
    fn call(&self, command: &str, payload: Value) -> Result<Value, String>;
}

/// Gerçek köprü: uygulama içi eklenti (`plugin::call`).
pub struct PluginBridge(AppHandle);

impl VaultBridge for PluginBridge {
    fn call(&self, command: &str, payload: Value) -> Result<Value, String> {
        crate::plugin::call(&self.0, command, payload)
    }
}

#[derive(Serialize)]
struct KeyArg<'a> {
    key: &'a str,
}

#[derive(Serialize)]
struct SetArg<'a> {
    key: &'a str,
    value: &'a str,
}

#[derive(Deserialize)]
struct ValueResponse {
    value: Option<String>,
}

fn vault_err(command: &str, e: impl std::fmt::Display) -> AppError {
    // Hata metni anahtar adını ya da değeri içermez; yalnız komut günlüğe yazılır.
    tracing::warn!(command, error = %e, "anahtar kasası hatası");
    AppError::new(
        ErrorCode::Keyring,
        "Android anahtar kasasına erişilemedi. Cihaz kilidini açıp yeniden dene; sürerse hesabı kaldırıp yeniden ekle.",
    )
}

/// `SecretStore`un Android uygulaması.
pub struct KeystoreSecrets<B: VaultBridge> {
    bridge: B,
}

pub type AndroidKeystoreSecrets = KeystoreSecrets<PluginBridge>;

impl AndroidKeystoreSecrets {
    /// Eklenti kayıtlı değilse (Android dışı) hata döner.
    pub fn new(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        if !cfg!(target_os = "android") {
            return Err("Android anahtar kasası yalnız Android'de var; masaüstünde MAILHERE_SECRETS=memory ile aç".into());
        }
        Ok(Self::with_bridge(PluginBridge(app.clone())))
    }
}

impl<B: VaultBridge> KeystoreSecrets<B> {
    pub fn with_bridge(bridge: B) -> Self {
        Self { bridge }
    }

    fn run<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> AppResult<T> {
        let payload = serde_json::to_value(payload).map_err(|e| vault_err(command, e))?;
        let out = self
            .bridge
            .call(command, payload)
            .map_err(|e| vault_err(command, e))?;
        serde_json::from_value(out).map_err(|e| vault_err(command, e))
    }
}

impl<B: VaultBridge> SecretStore for KeystoreSecrets<B> {
    fn get(&self, account_id: &str) -> AppResult<Option<String>> {
        let r: ValueResponse = self.run("secretGet", KeyArg { key: account_id })?;
        Ok(r.value)
    }

    fn set(&self, account_id: &str, secret: &str) -> AppResult<()> {
        self.run::<Value>(
            "secretSet",
            SetArg {
                key: account_id,
                value: secret,
            },
        )?;
        Ok(())
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        self.run::<Value>("secretDelete", KeyArg { key: account_id })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Kotlin kasasının JSON sözleşmesini taklit eder.
    #[derive(Default)]
    struct FakeVault {
        map: Mutex<HashMap<String, String>>,
        fail: bool,
    }

    impl VaultBridge for FakeVault {
        fn call(&self, command: &str, payload: Value) -> Result<Value, String> {
            if self.fail {
                return Err("Anahtar kasası okunamadı".into());
            }
            let key = payload["key"].as_str().ok_or("key yok")?.to_owned();
            let mut map = self.map.lock().unwrap();
            match command {
                "secretGet" => Ok(serde_json::json!({ "value": map.get(&key) })),
                "secretSet" => {
                    let value = payload["value"].as_str().ok_or("value yok")?;
                    map.insert(key, value.to_owned());
                    Ok(Value::Null)
                }
                "secretDelete" => {
                    map.remove(&key);
                    Ok(Value::Null)
                }
                other => Err(format!("bilinmeyen komut {other}")),
            }
        }
    }

    #[test]
    fn json_eslemesi_gidip_gelir() {
        let s = KeystoreSecrets::with_bridge(FakeVault::default());
        assert_eq!(s.get("hesap-1").unwrap(), None);
        s.set("hesap-1", "çok gizli").unwrap();
        s.set(mh_store_key(), "0123abcd").unwrap();
        assert_eq!(s.get("hesap-1").unwrap().as_deref(), Some("çok gizli"));
        assert_eq!(s.get(mh_store_key()).unwrap().as_deref(), Some("0123abcd"));
        s.delete("hesap-1").unwrap();
        assert_eq!(s.get("hesap-1").unwrap(), None);
        // Olmayan anahtarı silmek hata değildir.
        s.delete("yok").unwrap();
    }

    #[test]
    fn kasa_hatasi_degersiz_keyring_hatasina_doner() {
        let s = KeystoreSecrets::with_bridge(FakeVault {
            fail: true,
            ..Default::default()
        });
        let err = s.set("hesap-1", "parola123").unwrap_err();
        assert_eq!(err.code, ErrorCode::Keyring);
        assert!(!err.message.contains("parola123"));
        assert!(!err.message.contains("hesap-1"));
        assert_eq!(s.get("hesap-1").unwrap_err().code, ErrorCode::Keyring);
    }

    /// Veritabanı anahtarı da aynı kasada, `db-key` adıyla durur
    /// (`mh_store::DB_KEY_SECRET`).
    fn mh_store_key() -> &'static str {
        "db-key"
    }
}
