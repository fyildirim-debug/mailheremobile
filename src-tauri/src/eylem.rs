//! Bildirimden gelen eylemler: posta açma, Okundu, Arşivle, Sil, İstenmeyen,
//! satır içi Yanıtla ve yanıtı geri alma. Uygulama ekranı açılmadan
//! çekirdekte çalışır.
//!
//! Yol: bildirim düğmesi → `BildirimAlici` (Kotlin, `exported=false`) →
//! `ArkaKopru.eylem(json)` (JNI, `kopru.rs`) → [`dispatch_global`] → hangi
//! çekirdek açıksa (pencereli Tauri ya da etkinliksiz, `cekirdek.rs`) onun
//! çalışma ortamında `mh_app::notify_actions::{apply, reply}`. İşlemler
//! önce yerelde uygulanır, sunucuya kalıcı işlem kuyruğuyla gider: ağ yoksa
//! bağlantı gelince uygulanır. Bitince bildirim kalkar ve özet güncellenir;
//! olmazsa bildirimde "ne oldu + ne yapılabilir" yazar.
//!
//! Çekirdek yokken (süreç yeni açılıyor ya da hiç yok) `ArkaKopru.eylem`
//! `false` döner: Kotlin eylemi şifreli kasaya (`bildirim-bekleyen`) yazar,
//! bildirimi "Mailhere açılınca uygulanacak" durumuna çevirir ve hizmeti
//! başlatmayı dener; çekirdek açılınca ([`setup`] ya da
//! [`bekleyenleri_uygula_basliksiz`]) kuyruk boşaltılıp aynı yoldan uygulanır.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mh_app::headless::Headless;
use mh_core::{AppError, AppResult, MessageId, NotifyAction, OutboxItem};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

/// Soğuk açılışta arayüzün olay dinleyicileri sayfa yüklendikten sonra
/// kurulur; bildirimden açılan posta bu kadar bekletilir.
const UI_SETTLE: Duration = Duration::from_millis(1200);

/// Bildirimden gelen eylem (Kotlin `tur` alanı).
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "tur", rename_all = "camelCase")]
pub enum Eylem {
    /// Bildirime dokunuldu: posta açılır.
    Ac {
        id: MessageId,
    },
    Okundu {
        id: MessageId,
    },
    Arsivle {
        id: MessageId,
    },
    /// Çöp kutusuna taşır.
    Sil {
        id: MessageId,
    },
    /// Hesabın İstenmeyen kutusuna taşır.
    Istenmeyen {
        id: MessageId,
    },
    /// Satır içi yanıt (RemoteInput).
    Yanit {
        id: MessageId,
        metin: String,
    },
    /// Giden kutusundaki yanıtı gönderilmeden geri alır.
    GeriAl {
        id: MessageId,
        kuyruk: String,
    },
    /// Anlık bildirim açıkken periyodik uyanma: bağlantılar denetlenir.
    Uyan,
}

impl Eylem {
    fn kind(&self) -> &'static str {
        match self {
            Eylem::Ac { .. } => "ac",
            Eylem::Okundu { .. } => "okundu",
            Eylem::Arsivle { .. } => "arsivle",
            Eylem::Sil { .. } => "sil",
            Eylem::Istenmeyen { .. } => "istenmeyen",
            Eylem::Yanit { .. } => "yanit",
            Eylem::GeriAl { .. } => "geriAl",
            Eylem::Uyan => "uyan",
        }
    }

    /// Düğmenin çekirdek eylemi (yanıt, açma, uyanma hariç).
    pub fn action(&self) -> Option<(MessageId, NotifyAction)> {
        match self {
            Eylem::Okundu { id } => Some((*id, NotifyAction::MarkRead)),
            Eylem::Arsivle { id } => Some((*id, NotifyAction::Archive)),
            Eylem::Sil { id } => Some((*id, NotifyAction::Trash)),
            Eylem::Istenmeyen { id } => Some((*id, NotifyAction::Spam)),
            _ => None,
        }
    }
}

/// Arayüz hazır mı; değilse bildirimden açılacak posta bekletilir.
#[derive(Default)]
pub struct UiGate {
    ready: AtomicBool,
    pending: Mutex<Option<MessageId>>,
}

/// Ana pencerenin sayfası yüklendi: kısa bir süre sonra arayüz hazır sayılır
/// ve bekleyen posta açılır (yalnız ilk yüklemede beklenir).
pub fn page_loaded(app: &AppHandle) {
    let Some(gate) = app.try_state::<UiGate>() else {
        return;
    };
    if gate.ready.load(Ordering::Acquire) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(UI_SETTLE).await;
        let Some(gate) = app.try_state::<UiGate>() else {
            return;
        };
        gate.ready.store(true, Ordering::Release);
        let id = gate
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(id) = id {
            open(&app, id);
        }
    });
}

fn open(app: &AppHandle, id: MessageId) {
    if let Some(state) = app.try_state::<mh_app::AppState>() {
        state.platform.open_main(app, Some(id));
    }
}

/// Posta açma isteği: arayüz hazırsa hemen, değilse sayfa yüklenince.
fn open_when_ready(app: &AppHandle, id: MessageId) {
    let Some(gate) = app.try_state::<UiGate>() else {
        return open(app, id);
    };
    if gate.ready.load(Ordering::Acquire) {
        open(app, id);
    } else {
        *gate.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
    }
}

/// Kasadaki bekleyen eylemler (süreç kapalıyken basılan düğmeler, soğuk
/// açılıştaki dokunuş); alınınca silinir.
fn bekleyenler() -> Vec<Eylem> {
    let liste = match crate::kopru::bekleyen_eylemler() {
        Ok(l) => l,
        Err(e) => {
            if cfg!(target_os = "android") {
                tracing::warn!(error = %e, "bekleyen bildirim eylemleri okunamadı");
            }
            return Vec::new();
        }
    };
    liste
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<Eylem>(v) {
            Ok(e) => Some(e),
            Err(e) => {
                tracing::warn!(error = %e, "bekleyen eylem okunamadı");
                None
            }
        })
        .collect()
}

/// Pencereli çekirdek açıldı: bekleyen eylemler uygulanır. `setup_core`dan
/// sonra, ana pencere açılmadan çağrılır.
pub fn setup(app: &AppHandle) {
    app.manage(UiGate::default());
    for e in bekleyenler() {
        dispatch(app, e);
    }
}

/// Etkinliksiz çekirdek açıldı: bekleyen eylemler onda uygulanır (dokunuş
/// pencere ister; etkinlik gelince yeniden okunmak üzere bırakılmaz,
/// kullanıcı bildirime yeniden dokunur).
pub fn bekleyenleri_uygula_basliksiz(h: &Arc<Headless>) {
    for e in bekleyenler() {
        if matches!(e, Eylem::Ac { .. }) {
            continue;
        }
        dispatch_basliksiz(h, e);
    }
}

/// Kotlin'den gelen eylem hangi çekirdek açıksa ona gider; çekirdek yoksa
/// `false` (Kotlin kasada bekletir).
pub fn dispatch_global(e: Eylem) -> bool {
    if let Some(app) = crate::cekirdek::tauri() {
        dispatch(&app, e);
        return true;
    }
    if let Some(h) = crate::cekirdek::basliksiz() {
        if matches!(e, Eylem::Ac { .. }) {
            // Pencere yok: dokunuş etkinliği açar, posta oradan açılır.
            return false;
        }
        dispatch_basliksiz(&h, e);
        return true;
    }
    false
}

/// Eylemi pencereli çekirdeğin çalışma ortamına verir.
fn dispatch(app: &AppHandle, e: Eylem) {
    tracing::debug!(eylem = e.kind(), "bildirim eylemi");
    if let Eylem::Ac { id } = e {
        return open_when_ready(app, id);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(&app, e).await });
}

/// Eylemi etkinliksiz çekirdeğin çalışma ortamına verir.
fn dispatch_basliksiz(h: &Arc<Headless>, e: Eylem) {
    tracing::debug!(eylem = e.kind(), "bildirim eylemi (etkinliksiz)");
    let h = h.clone();
    h.handle()
        .clone()
        .spawn(async move { run_basliksiz(h, e).await });
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(mh_app::state::internal)?
}

/// Pencereli çekirdekte eylem.
async fn run(app: &AppHandle, e: Eylem) {
    if let Some((id, action)) = e.action() {
        let h = app.clone();
        let res = blocking(move || {
            let state = h.state::<mh_app::AppState>();
            mh_app::notify_actions::apply(&h, &state, id, action)
        })
        .await;
        return done(id, res).await;
    }
    match e {
        Eylem::Ac { id } => open_when_ready(app, id),
        Eylem::Yanit { id, metin } => {
            let res = mh_app::notify_actions::reply(app, id, &metin).await;
            yanit_sonucu(id, &metin, res).await;
        }
        Eylem::GeriAl { id, kuyruk } => {
            let Some(state) = app.try_state::<mh_app::AppState>() else {
                return;
            };
            let outbox = state.outbox.clone();
            geri_al(id, blocking(move || outbox.cancel(&kuyruk)).await).await;
        }
        Eylem::Uyan => {
            // Uykuda yarı ölü kalmış bağlantılar bir eşitlemeyle sınanır;
            // kopmuşsa motor yeniden bağlanır, yeni posta bildirilir.
            if let Some(state) = app.try_state::<mh_app::AppState>() {
                state.sync.sync_now();
                state.jmap.sync_now();
            }
        }
        Eylem::Okundu { .. }
        | Eylem::Arsivle { .. }
        | Eylem::Sil { .. }
        | Eylem::Istenmeyen { .. } => {}
    }
}

/// Etkinliksiz çekirdekte eylem (kendi çalışma ortamında).
async fn run_basliksiz(h: Arc<Headless>, e: Eylem) {
    if let Some((id, action)) = e.action() {
        let h2 = h.clone();
        let res = blocking(move || h2.apply(id, action)).await;
        return done(id, res).await;
    }
    match e {
        Eylem::Yanit { id, metin } => {
            let res = h.reply(id, &metin).await;
            yanit_sonucu(id, &metin, res).await;
        }
        Eylem::GeriAl { id, kuyruk } => {
            let outbox = h.state().outbox.clone();
            geri_al(id, blocking(move || outbox.cancel(&kuyruk)).await).await;
        }
        Eylem::Uyan => h.sync_now(),
        Eylem::Ac { .. }
        | Eylem::Okundu { .. }
        | Eylem::Arsivle { .. }
        | Eylem::Sil { .. }
        | Eylem::Istenmeyen { .. } => {}
    }
}

/// Düğme sonucu: başarıda bildirim kalkar (özet güncellenir), hatada nedeni
/// ve çözümü yazar.
async fn done(id: MessageId, res: AppResult<()>) {
    match res {
        Ok(()) => {
            let r = blocking(move || {
                crate::kopru::bildirim_kaldir(&[id]).map_err(|e| {
                    AppError::new(
                        mh_core::ErrorCode::Internal,
                        format!("bildirim kaldırılamadı: {e}"),
                    )
                })
            })
            .await;
            if let Err(e) = r {
                tracing::warn!(error = %e.message, "bildirim kaldırılamadı");
            }
        }
        Err(e) => failed(id, &e).await,
    }
}

async fn failed(id: MessageId, e: &AppError) {
    tracing::warn!(code = %e.code, error = %e.message, "bildirim eylemi uygulanamadı");
    let metin = e.message.clone();
    let r = blocking(move || {
        crate::kopru::bildirim_hata(id, &metin)
            .map_err(|e| AppError::new(mh_core::ErrorCode::Internal, e))
    })
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e.message, "bildirim güncellenemedi");
    }
}

/// Yanıt giden kutusuna girdi: bildirim geri sayımlı "Geri al" satırına döner.
async fn yanit_sonucu(id: MessageId, metin: &str, res: AppResult<OutboxItem>) {
    match res {
        Ok(item) => {
            let metin = metin.trim().to_owned();
            let r = blocking(move || {
                crate::kopru::yanit_kuyrukta(id, &item.id, item.send_at, &metin)
                    .map_err(|e| AppError::new(mh_core::ErrorCode::Internal, e))
            })
            .await;
            if let Err(e) = r {
                tracing::warn!(error = %e.message, "yanıt bildirimi güncellenemedi");
            }
        }
        Err(e) => failed(id, &e).await,
    }
}

async fn geri_al(id: MessageId, res: AppResult<bool>) {
    match res {
        Ok(iptal) => {
            let r = blocking(move || {
                crate::kopru::yanit_geri_alindi(id, iptal)
                    .map_err(|e| AppError::new(mh_core::ErrorCode::Internal, e))
            })
            .await;
            if let Err(e) = r {
                tracing::warn!(error = %e.message, "yanıt bildirimi güncellenemedi");
            }
        }
        Err(e) => failed(id, &e).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eylemler_kotlin_bicimini_okur() {
        let e: Eylem = serde_json::from_str(r#"{"tur":"okundu","id":5,"hesap":"is"}"#).unwrap();
        assert_eq!(e, Eylem::Okundu { id: 5 });
        assert_eq!(e.action(), Some((5, NotifyAction::MarkRead)));
        let e: Eylem = serde_json::from_str(r#"{"tur":"arsivle","id":6}"#).unwrap();
        assert_eq!(e.action(), Some((6, NotifyAction::Archive)));
        let e: Eylem = serde_json::from_str(r#"{"tur":"sil","id":1}"#).unwrap();
        assert_eq!(e, Eylem::Sil { id: 1 });
        assert_eq!(e.action(), Some((1, NotifyAction::Trash)));
        let e: Eylem = serde_json::from_str(r#"{"tur":"istenmeyen","id":2}"#).unwrap();
        assert_eq!(e.action(), Some((2, NotifyAction::Spam)));
        let e: Eylem = serde_json::from_str(r#"{"tur":"yanit","id":7,"metin":"Tamam"}"#).unwrap();
        assert_eq!(
            e,
            Eylem::Yanit {
                id: 7,
                metin: "Tamam".into()
            }
        );
        assert_eq!(e.action(), None);
        let e: Eylem = serde_json::from_str(r#"{"tur":"geriAl","id":7,"kuyruk":"u-1"}"#).unwrap();
        assert_eq!(
            e,
            Eylem::GeriAl {
                id: 7,
                kuyruk: "u-1".into()
            }
        );
        let e: Eylem = serde_json::from_str(r#"{"tur":"ac","id":8}"#).unwrap();
        assert_eq!(e, Eylem::Ac { id: 8 });
        assert_eq!(e.kind(), "ac");
        let e: Eylem = serde_json::from_str(r#"{"tur":"uyan"}"#).unwrap();
        assert_eq!(e, Eylem::Uyan);
        assert!(serde_json::from_str::<Eylem>(r#"{"tur":"yok","id":1}"#).is_err());
    }

    #[test]
    fn cekirdek_yokken_eylem_bekletilir() {
        // Masaüstü testinde kapı boştur: Kotlin eylemi kasaya yazar.
        assert!(!dispatch_global(Eylem::Sil { id: 1 }));
        assert!(!dispatch_global(Eylem::Uyan));
        assert!(bekleyenler().is_empty());
    }
}
