//! Süreç başına tek çekirdek: pencereli (Tauri, `setup_core`) ya da
//! etkinliksiz (`mh_app::headless::Headless`) çekirdekten yalnız biri açık
//! olur; iki `Store` aynı SQLCipher veritabanını açmaz.
//!
//! Kapı ([`Kapi`]) dört durumlu bir kilittir:
//!
//! - `Yok` — çekirdek yok.
//! - `BasliksizKuruluyor` — hizmet/iş etkinliksiz çekirdeği açıyor (depo
//!   açılıyor, birkaç yüz ms). Bu sırada gelen Tauri kurulumu bitmesini
//!   bekler; biten etkinliksiz çekirdek kapatılıp Tauri'ye yer açılır.
//! - `Basliksiz(h)` — etkinliksiz çekirdek çalışıyor (hizmet açıkken ya da
//!   tek seferlik yoklamada).
//! - `TauriKuruluyor` — etkinlik geldi, `setup_core` çalışıyor; etkinliksiz
//!   çekirdek varsa alınıp durduruldu. Bu sırada etkinliksiz başlatma
//!   isteği "dolu" yanıtı alır.
//! - `Tauri(app)` — pencereli çekirdek (etkinlik yok edilmiş olsa da süreç
//!   yaşıyorsa çekirdek budur; `lib.rs` çıkışı engeller).
//!
//! Kurallar: etkinliksiz çekirdek yalnız `Yok`tan açılır; Tauri her durumda
//! önceliklidir (etkinliksizi durdurur, kuruluyorsa bitmesini bekler); Tauri
//! açıkken etkinliksiz istek yalnız `sync_now` tetikler. Bildirim eylemleri
//! ve uyanma hangi çekirdek açıksa ona gider; hiçbiri yoksa `false` döner
//! ve Kotlin eylemi kasada bekletir. Durum makinesi birim testlerle
//! kanıtlanır (`Kapi` tür parametrelidir; testler `AppHandle` gerektirmez).

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use mh_app::headless::{Headless, HeadlessShell};
use mh_app::AppState;
use mh_core::{NotifyEvent, Settings};
use tauri::{AppHandle, Manager};

/// Çekirdeğin dışa görünen durumu (`ArkaKopru.cekirdekDurumu`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durum {
    Yok,
    Basliksiz,
    Tauri,
}

impl Durum {
    pub fn kod(self) -> i32 {
        match self {
            Durum::Yok => 0,
            Durum::Basliksiz => 1,
            Durum::Tauri => 2,
        }
    }
}

enum Aktif<T, H> {
    Yok,
    BasliksizKuruluyor,
    Basliksiz(H),
    TauriKuruluyor,
    Tauri(T),
}

/// Etkinliksiz başlatmanın sonucu (JNI dönüş kodu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baslatma {
    /// Çekirdek başladı (tek seferde: çalıştı ve kapandı).
    Basladi,
    /// Zaten bir çekirdek var (Tauri ya da etkinliksiz); yalnız eşitleme
    /// tetiklendi.
    Zaten,
    /// JNI köprüsü kurulmamış (`Application.onCreate` çalışmadı).
    KopruYok,
    /// Kasada veritabanı anahtarı yok ya da kasa okunamadı: ilk açılış
    /// uygulamadan yapılmalı; anahtar üretilmez.
    AnahtarYok,
    /// Hesap yok: yapılacak iş yok, çekirdek kapatıldı.
    HesapYok,
    /// Depo açılamadı ya da eşitleme başlatılamadı (günlükte).
    Hata,
    /// Kurulum bitince Tauri çekirdeğinin kurulduğu görüldü; etkinliksiz
    /// çekirdek hiç başlatılmadan kapatıldı.
    Vazgecildi,
    /// Parola kilidi açık: anahtar parolayla sarılı, çekirdek yalnız
    /// uygulamadan parola girilince açılır. "Mailhere kilitli" bildirimi
    /// gösterildi; hizmet ve iş başka bir şey yapmaz.
    Kilitli,
}

impl Baslatma {
    pub fn kod(self) -> i32 {
        match self {
            Baslatma::Basladi => 0,
            Baslatma::Zaten => 1,
            Baslatma::KopruYok => 2,
            Baslatma::AnahtarYok => 3,
            Baslatma::HesapYok => 4,
            Baslatma::Hata => 5,
            Baslatma::Vazgecildi => 6,
            Baslatma::Kilitli => 7,
        }
    }
}

/// Tauri kurulumunun, açılmakta olan etkinliksiz çekirdeği beklediği en
/// uzun süre; sonra kapı zorla alınır (açılış biten çekirdek kendini kapatır).
const KURULUM_BEKLEME: Duration = Duration::from_secs(15);

/// Kapı: durum + koşul değişkeni (`BasliksizKuruluyor` bitişini duyurur).
pub struct Kapi<T, H> {
    aktif: Mutex<Aktif<T, H>>,
    degisti: Condvar,
}

impl<T: Clone, H: Clone> Default for Kapi<T, H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone, H: Clone> Kapi<T, H> {
    pub const fn new() -> Self {
        Self {
            aktif: Mutex::new(Aktif::Yok),
            degisti: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Aktif<T, H>> {
        self.aktif.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn durum(&self) -> Durum {
        match &*self.lock() {
            Aktif::Yok | Aktif::BasliksizKuruluyor | Aktif::TauriKuruluyor => Durum::Yok,
            Aktif::Basliksiz(_) => Durum::Basliksiz,
            Aktif::Tauri(_) => Durum::Tauri,
        }
    }

    pub fn tauri(&self) -> Option<T> {
        match &*self.lock() {
            Aktif::Tauri(t) => Some(t.clone()),
            _ => None,
        }
    }

    pub fn basliksiz(&self) -> Option<H> {
        match &*self.lock() {
            Aktif::Basliksiz(h) => Some(h.clone()),
            _ => None,
        }
    }

    /// Etkinliksiz çekirdek açılmak isteniyor. Yalnız `Yok`tan: kapı
    /// `BasliksizKuruluyor`a geçer, kilit bırakılır, `kur` çalışır. Sonuç:
    /// `Ok(Some(h))` açıldı; `Ok(None)` kapı doluydu (`kur` çalışmadı);
    /// `Err(Vazgecildi(h))` `kur` bitince Tauri'nin kapıyı almış olduğu
    /// görüldü — çağıran `h`yi kapatır; `Err(Hata(e))` `kur` başarısız.
    pub fn basliksiz_ac<E>(
        &self,
        kur: impl FnOnce() -> Result<H, E>,
    ) -> Result<Option<H>, Vazgecis<H, E>> {
        {
            let mut a = self.lock();
            if !matches!(*a, Aktif::Yok) {
                return Ok(None);
            }
            *a = Aktif::BasliksizKuruluyor;
        }
        let sonuc = kur();
        let mut a = self.lock();
        let out = match sonuc {
            Ok(h) => {
                if matches!(*a, Aktif::BasliksizKuruluyor) {
                    *a = Aktif::Basliksiz(h.clone());
                    Ok(Some(h))
                } else {
                    // Bekleme süresi dolan Tauri kurulumu kapıyı aldı.
                    Err(Vazgecis::Vazgecildi(h))
                }
            }
            Err(e) => {
                if matches!(*a, Aktif::BasliksizKuruluyor) {
                    *a = Aktif::Yok;
                }
                Err(Vazgecis::Hata(e))
            }
        };
        self.degisti.notify_all();
        out
    }

    /// Pencereli çekirdek kuruluyor: varsa etkinliksiz çekirdek geri verilir
    /// (çağıran durdurur), açılmaktaysa bitmesi beklenir (en çok
    /// `KURULUM_BEKLEME`); kapı `TauriKuruluyor` olur.
    pub fn tauri_kuruluyor(&self) -> Option<H> {
        let mut a = self.lock();
        let son = Instant::now() + KURULUM_BEKLEME;
        loop {
            match std::mem::replace(&mut *a, Aktif::TauriKuruluyor) {
                Aktif::Basliksiz(h) => return Some(h),
                Aktif::BasliksizKuruluyor => {
                    *a = Aktif::BasliksizKuruluyor;
                    let kalan = son.saturating_duration_since(Instant::now());
                    if kalan.is_zero() {
                        // Zorla al: biten açılış `Vazgecildi` alır ve kapanır.
                        *a = Aktif::TauriKuruluyor;
                        return None;
                    }
                    let (g, _) = self
                        .degisti
                        .wait_timeout(a, kalan)
                        .unwrap_or_else(|e| e.into_inner());
                    a = g;
                }
                _ => return None,
            }
        }
    }

    /// `setup_core` bitti: pencereli çekirdek hazır.
    pub fn tauri_hazir(&self, t: T) {
        *self.lock() = Aktif::Tauri(t);
        self.degisti.notify_all();
    }

    /// `setup_core` başarısız: kapı boşalır.
    pub fn tauri_vazgec(&self) {
        let mut a = self.lock();
        if matches!(*a, Aktif::TauriKuruluyor) {
            *a = Aktif::Yok;
        }
        self.degisti.notify_all();
    }

    /// Etkinliksiz çekirdeği kapıdan alır (durdurmak için); kapı boşalır.
    pub fn basliksiz_al(&self) -> Option<H> {
        let mut a = self.lock();
        match std::mem::replace(&mut *a, Aktif::Yok) {
            Aktif::Basliksiz(h) => {
                self.degisti.notify_all();
                Some(h)
            }
            other => {
                *a = other;
                None
            }
        }
    }
}

/// `Kapi::basliksiz_ac` hata kolu.
pub enum Vazgecis<H, E> {
    Vazgecildi(H),
    Hata(E),
}

/// Süreçteki kapı: Tauri tutamacı ya da etkinliksiz çekirdek.
static KAPI: Kapi<AppHandle, Arc<Headless>> = Kapi::new();

/// Tauri kurulumunun başı (`lib.rs::setup`): varsa etkinliksiz çekirdek
/// durdurulur.
pub fn tauri_kuruluyor() {
    if let Some(h) = KAPI.tauri_kuruluyor() {
        tracing::info!("etkinlik geldi; etkinliksiz çekirdek pencereliye yer açıyor");
        h.stop();
    }
}

pub fn tauri_hazir(app: AppHandle) {
    KAPI.tauri_hazir(app);
}

pub fn tauri_vazgec() {
    KAPI.tauri_vazgec();
}

pub fn durum() -> Durum {
    KAPI.durum()
}

pub fn tauri() -> Option<AppHandle> {
    KAPI.tauri()
}

pub fn basliksiz() -> Option<Arc<Headless>> {
    KAPI.basliksiz()
}

/// Açık çekirdeğin durumuyla bir iş; çekirdek yoksa `None`.
pub fn with_state<R>(f: impl FnOnce(&AppState) -> R) -> Option<R> {
    if let Some(h) = KAPI.basliksiz() {
        return Some(f(h.state()));
    }
    let app = KAPI.tauri()?;
    let state = app.try_state::<AppState>()?;
    Some(f(&state))
}

/// Hangi çekirdek açıksa ona "şimdi eşitle"; çekirdek yoksa `false`.
pub fn sync_now() -> bool {
    with_state(|state| {
        state.sync.sync_now();
        state.jmap.sync_now();
    })
    .is_some()
}

/// Etkinliksiz çekirdeğin kabuğu: bildirimleri Kotlin'e köprüden verir.
struct AndroidKabuk;

impl HeadlessShell for AndroidKabuk {
    fn notify(&self, ev: NotifyEvent, settings: &Settings) {
        let has_junk = with_state(|state| {
            mh_app::notify_actions::has_junk(state, &ev.message.account_id).unwrap_or(false)
        })
        .unwrap_or(false);
        crate::notify::show(&ev, crate::notify::eylem_adlari(settings, has_junk));
    }

    fn counts_changed(&self) {
        crate::notify::reconcile_soon();
    }
}

fn ac() -> Result<Arc<Headless>, Baslatma> {
    let dirs = crate::kopru::dirs().ok_or(Baslatma::KopruYok)?;
    let secrets = Arc::new(crate::keystore::AndroidKeystoreSecrets::new().map_err(|e| {
        tracing::error!(error = %e, "etkinliksiz çekirdek: kasa yok");
        Baslatma::KopruYok
    })?);
    match Headless::open(&dirs, secrets, Arc::new(AndroidKabuk)) {
        Ok(h) => Ok(h),
        Err(e) if e.code == mh_core::ErrorCode::Locked => {
            tracing::info!("etkinliksiz çekirdek: parola kilidi açık, çekirdek açılmadı");
            kilitli_bildir();
            Err(Baslatma::Kilitli)
        }
        Err(e) if e.code == mh_core::ErrorCode::Keyring => {
            tracing::warn!(error = %e.message, "etkinliksiz çekirdek açılmadı");
            Err(Baslatma::AnahtarYok)
        }
        Err(e) => {
            tracing::error!(code = %e.code, error = %e.message, "etkinliksiz çekirdek açılamadı");
            Err(Baslatma::Hata)
        }
    }
}

/// Kilitli açılışta sessiz "Mailhere kilitli — postaları almak için bir kez
/// aç" bildirimi (Kotlin `Bildirimler.kilitliGoster`; aynı kimlik, yeniden
/// gösterim sessizce günceller). Kilit açılınca `Platform::core_ready`
/// kaldırır.
fn kilitli_bildir() {
    if let Err(e) = crate::kopru::kilitli_goster() {
        tracing::warn!(error = %e, "kilit bildirimi gösterilemedi");
    }
}

/// Etkinliksiz çekirdeği açar ve başlatır (hizmet). Hesap yoksa açılan
/// çekirdek hemen kapatılır. Parola kilidi açıksa çekirdek açılmaz
/// (`Kilitli`): postalar ancak uygulamada parola girilince gelir.
pub fn basliksiz_baslat() -> Baslatma {
    if !crate::kopru::kuruldu() {
        return Baslatma::KopruYok;
    }
    match KAPI.basliksiz_ac(ac) {
        Ok(None) => {
            sync_now();
            Baslatma::Zaten
        }
        Ok(Some(h)) => {
            if h.account_count() == 0 {
                tracing::info!("etkinliksiz çekirdek: hesap yok, kapatıldı");
                basliksiz_durdur();
                return Baslatma::HesapYok;
            }
            if let Err(e) = h.start() {
                tracing::error!(code = %e.code, error = %e.message, "etkinliksiz eşitleme başlatılamadı");
                basliksiz_durdur();
                return Baslatma::Hata;
            }
            tracing::info!(hesap = h.account_count(), "etkinliksiz çekirdek çalışıyor");
            // Süreç kapalıyken basılan düğmeler şimdi uygulanır.
            crate::eylem::bekleyenleri_uygula_basliksiz(&h);
            Baslatma::Basladi
        }
        Err(Vazgecis::Vazgecildi(h)) => {
            tracing::info!("etkinliksiz çekirdek açılırken etkinlik geldi; kapatıldı");
            h.stop();
            Baslatma::Vazgecildi
        }
        Err(Vazgecis::Hata(kod)) => kod,
    }
}

/// Etkinliksiz çekirdeği durdurur (hizmet kapandı ya da Tauri geliyor).
pub fn basliksiz_durdur() -> bool {
    match KAPI.basliksiz_al() {
        Some(h) => {
            h.stop();
            true
        }
        None => false,
    }
}

/// Yoklama adımı ve bildirimlerin gösterilmesi için pay.
const ADIM: Duration = Duration::from_millis(500);
const PAY: Duration = Duration::from_secs(3);
/// Var olan çekirdeğe yalnız eşitleme tetiklenirken (iş uyanıklığı) beklenen süre.
const TETIK_BEKLEME: Duration = Duration::from_secs(20);

/// Tek seferlik eşitle-bildir (JobScheduler yedeği; süreç etkinliksiz ve
/// hizmetsizken 15 dk'da bir). Çekirdek varsa yalnız eşitleme tetikler ve
/// kısa süre bekler (iş uyanıklığı eşitlemeyi kapsasın). Yoksa etkinliksiz
/// çekirdeği açar, tüm hesaplar ilk eşitlemeyi bitirip bildirimler
/// gösterilince (ya da süre dolunca) kapatır. Bu sırada etkinlik gelirse
/// çekirdek Tauri'ye devredilir ve iş hemen biter.
pub fn tek_sefer(sure: Duration) -> Baslatma {
    let son = Instant::now() + sure;
    match basliksiz_baslat() {
        Baslatma::Basladi => {}
        Baslatma::Zaten => {
            let kadar = sure.min(TETIK_BEKLEME);
            let bitis = Instant::now() + kadar;
            while Instant::now() < bitis {
                std::thread::sleep(ADIM);
                if let Some(h) = KAPI.basliksiz() {
                    if h.quiet() {
                        std::thread::sleep(
                            PAY.min(bitis.saturating_duration_since(Instant::now())),
                        );
                        break;
                    }
                }
            }
            return Baslatma::Zaten;
        }
        other => return other,
    }
    let mut sessiz_an: Option<Instant> = None;
    loop {
        std::thread::sleep(ADIM);
        let Some(h) = KAPI.basliksiz() else {
            // Etkinlik geldi, çekirdek Tauri'ye devredildi.
            return Baslatma::Basladi;
        };
        let simdi = Instant::now();
        if simdi >= son {
            tracing::info!("tek seferlik eşitleme: süre doldu");
            break;
        }
        if h.quiet() {
            let an = *sessiz_an.get_or_insert(simdi);
            if simdi.duration_since(an) >= PAY {
                tracing::info!("tek seferlik eşitleme bitti");
                break;
            }
        } else {
            sessiz_an = None;
        }
    }
    basliksiz_durdur();
    Baslatma::Basladi
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    type K = Kapi<u8, String>;

    #[test]
    fn bos_kapidan_etkinliksiz_acilir_ve_alinir() {
        let k = K::new();
        assert_eq!(k.durum(), Durum::Yok);
        let h = k
            .basliksiz_ac(|| Ok::<_, ()>("h1".to_owned()))
            .ok()
            .flatten();
        assert_eq!(h.as_deref(), Some("h1"));
        assert_eq!(k.durum(), Durum::Basliksiz);
        assert_eq!(k.basliksiz().as_deref(), Some("h1"));
        // İkinci açılış kapıyı dolu bulur; kurucu çalışmaz.
        let calisti = AtomicUsize::new(0);
        let r = k.basliksiz_ac(|| {
            calisti.fetch_add(1, Ordering::SeqCst);
            Ok::<_, ()>("h2".to_owned())
        });
        assert!(matches!(r, Ok(None)));
        assert_eq!(calisti.load(Ordering::SeqCst), 0);
        assert_eq!(k.basliksiz_al().as_deref(), Some("h1"));
        assert_eq!(k.durum(), Durum::Yok);
        assert!(k.basliksiz_al().is_none());
    }

    #[test]
    fn kurucu_hatasi_kapiyi_bosaltir() {
        let k = K::new();
        let r = k.basliksiz_ac(|| Err::<String, _>("depo"));
        assert!(matches!(r, Err(Vazgecis::Hata("depo"))));
        assert_eq!(k.durum(), Durum::Yok);
        assert!(k.basliksiz_ac(|| Ok::<_, ()>("h".to_owned())).is_ok());
    }

    #[test]
    fn tauri_etkinliksizi_alir_ve_oncelik_tauridedir() {
        let k = K::new();
        k.basliksiz_ac(|| Ok::<_, ()>("h".to_owned())).ok();
        // Etkinlik geldi: etkinliksiz çekirdek durdurulmak üzere geri verilir.
        assert_eq!(k.tauri_kuruluyor().as_deref(), Some("h"));
        assert_eq!(k.durum(), Durum::Yok, "kuruluyor: dışa 'yok' görünür");
        // Kurulum sürerken etkinliksiz istek dolu yanıtı alır.
        assert!(matches!(
            k.basliksiz_ac(|| Ok::<_, ()>("h2".to_owned())),
            Ok(None)
        ));
        k.tauri_hazir(7);
        assert_eq!(k.durum(), Durum::Tauri);
        assert_eq!(k.tauri(), Some(7));
        assert!(k.basliksiz().is_none());
        assert!(matches!(
            k.basliksiz_ac(|| Ok::<_, ()>("h3".to_owned())),
            Ok(None)
        ));
        // Tauri açıkken etkinliksiz "al" bir şey vermez, durum değişmez.
        assert!(k.basliksiz_al().is_none());
        assert_eq!(k.durum(), Durum::Tauri);
    }

    #[test]
    fn tauri_kurulumu_basarisizsa_kapi_bosalir() {
        let k = K::new();
        assert!(k.tauri_kuruluyor().is_none());
        k.tauri_vazgec();
        assert_eq!(k.durum(), Durum::Yok);
        assert!(k.basliksiz_ac(|| Ok::<_, ()>("h".to_owned())).is_ok());
    }

    #[test]
    fn tauri_acilmakta_olan_etkinliksizi_bekler_ve_alir() {
        let k = Arc::new(K::new());
        let k2 = k.clone();
        let t = thread::spawn(move || {
            k2.basliksiz_ac(|| {
                thread::sleep(Duration::from_millis(200));
                Ok::<_, ()>("gec".to_owned())
            })
        });
        // Kurulum başlasın.
        thread::sleep(Duration::from_millis(50));
        assert_eq!(k.durum(), Durum::Yok);
        // Tauri bekler, biten çekirdeği alır.
        let alinan = k.tauri_kuruluyor();
        assert_eq!(alinan.as_deref(), Some("gec"));
        let r = t.join().expect("iş parçacığı");
        assert!(matches!(r, Ok(Some(ref h)) if h == "gec"));
        k.tauri_hazir(1);
        assert_eq!(k.durum(), Durum::Tauri);
    }

    #[test]
    fn kodlar_kotlin_ile_sabit() {
        assert_eq!(Durum::Yok.kod(), 0);
        assert_eq!(Durum::Basliksiz.kod(), 1);
        assert_eq!(Durum::Tauri.kod(), 2);
        assert_eq!(Baslatma::Basladi.kod(), 0);
        assert_eq!(Baslatma::Zaten.kod(), 1);
        assert_eq!(Baslatma::KopruYok.kod(), 2);
        assert_eq!(Baslatma::AnahtarYok.kod(), 3);
        assert_eq!(Baslatma::HesapYok.kod(), 4);
        assert_eq!(Baslatma::Hata.kod(), 5);
        assert_eq!(Baslatma::Vazgecildi.kod(), 6);
        assert_eq!(Baslatma::Kilitli.kod(), 7);
    }

    /// Kilitli yol: `Headless::open` `Locked` döndürünce çekirdek açılmaz,
    /// sonuç `Kilitli`dir ve kapı boş kalır (masaüstünde köprü olmadığından
    /// `KopruYok`; eşleme `ac()` ile aynı koldur).
    #[test]
    fn kilitli_hata_kilitli_koduna_eslenir() {
        let e = mh_core::AppError::new(mh_core::ErrorCode::Locked, "kilitli");
        let kod = match e.code {
            mh_core::ErrorCode::Locked => Baslatma::Kilitli,
            mh_core::ErrorCode::Keyring => Baslatma::AnahtarYok,
            _ => Baslatma::Hata,
        };
        assert_eq!(kod, Baslatma::Kilitli);
        assert_eq!(kod.kod(), 7, "Kotlin KOD_KILITLI");
        // Kilitli sonuç kapıyı doldurmaz: sonraki etkinliksiz açılış yine denenebilir.
        let k = Kapi::<u8, String>::new();
        let r = k.basliksiz_ac(|| Err::<String, _>(Baslatma::Kilitli));
        assert!(matches!(r, Err(Vazgecis::Hata(Baslatma::Kilitli))));
        assert_eq!(k.durum(), Durum::Yok);
    }

    #[test]
    fn masaustunde_etkinliksiz_cekirdek_acilmaz() {
        assert_eq!(basliksiz_baslat(), Baslatma::KopruYok);
        assert_eq!(tek_sefer(Duration::from_secs(1)), Baslatma::KopruYok);
        assert!(!basliksiz_durdur());
        assert!(!sync_now());
        assert_eq!(durum(), Durum::Yok);
    }
}
