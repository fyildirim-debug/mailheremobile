//! Android anahtar kasası köprüsü: parolalar, OAuth yenileme jetonları ve
//! veritabanı anahtarı (`db-key`) `SecretVault.kt`te, `AndroidKeyStore` ile
//! korunan AES-256-GCM anahtarıyla şifrelenir ve `mailhere-kasa` tercih
//! dosyasında durur. Kasaya etkinliksiz JNI köprüsünden gidilir
//! (`kopru.rs` → `ArkaKopru.kasaOku/kasaYaz/kasaSil`): her IMAP bağlantısı
//! bir okuma yapar ve bu, pencere yok edildikten sonra ya da hiç pencere
//! açılmadan da olur. Buradaki kod yalnız hata eşlemesi yapar; değerler
//! günlüğe ya da hata metnine yazılmaz.

use mh_core::{AppError, AppResult, ErrorCode};
use mh_sync::SecretStore;

/// Kotlin tarafına giden çağrılar; testte sahte kasayla değiştirilir.
pub trait VaultBridge: Send + Sync + 'static {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// Gerçek köprü: JNI (`kopru.rs`).
pub struct KopruBridge;

impl VaultBridge for KopruBridge {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        crate::kopru::kasa_oku(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        crate::kopru::kasa_yaz(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        crate::kopru::kasa_sil(key)
    }
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

pub type AndroidKeystoreSecrets = KeystoreSecrets<KopruBridge>;

impl AndroidKeystoreSecrets {
    /// JNI köprüsü kurulu değilse (Android dışı ya da `Application.onCreate`
    /// çalışmadı) hata döner.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        if !crate::kopru::kuruldu() {
            return Err("Android anahtar kasası yalnız Android'de var; masaüstünde MAILHERE_SECRETS=memory ile aç".into());
        }
        Ok(Self::with_bridge(KopruBridge))
    }
}

impl<B: VaultBridge> KeystoreSecrets<B> {
    pub fn with_bridge(bridge: B) -> Self {
        Self { bridge }
    }
}

impl<B: VaultBridge> SecretStore for KeystoreSecrets<B> {
    fn get(&self, account_id: &str) -> AppResult<Option<String>> {
        self.bridge
            .get(account_id)
            .map_err(|e| vault_err("kasaOku", e))
    }

    fn set(&self, account_id: &str, secret: &str) -> AppResult<()> {
        self.bridge
            .set(account_id, secret)
            .map_err(|e| vault_err("kasaYaz", e))
    }

    fn delete(&self, account_id: &str) -> AppResult<()> {
        self.bridge
            .delete(account_id)
            .map_err(|e| vault_err("kasaSil", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Kotlin kasasının sözleşmesini taklit eder.
    #[derive(Default)]
    struct FakeVault {
        map: Mutex<HashMap<String, String>>,
        fail: bool,
    }

    impl VaultBridge for FakeVault {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            if self.fail {
                return Err("Anahtar kasası okunamadı".into());
            }
            Ok(self.map.lock().unwrap().get(key).cloned())
        }

        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            if self.fail {
                return Err("Anahtar kasası yazılamadı".into());
            }
            self.map.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    #[test]
    fn kasa_gidip_gelir() {
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

    #[test]
    fn masaustunde_kasa_yok() {
        assert!(AndroidKeystoreSecrets::new().is_err());
    }

    /// Veritabanı anahtarı da aynı kasada, `db-key` adıyla durur
    /// (`mh_store::DB_KEY_SECRET`).
    fn mh_store_key() -> &'static str {
        "db-key"
    }
}
