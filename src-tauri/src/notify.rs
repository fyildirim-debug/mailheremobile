//! Sistem bildirimi: Android 13+ izin isteği ve yeni posta bildirimleri.
//!
//! Bildirimi Kotlin tarafı (`Bildirimler.kt`, `NotificationCompat`) çizer;
//! buradan yalnız veri gider — etkinlik olsa da olmasa da JNI köprüsünden
//! (`kopru.rs`, `ArkaKopru.bildir`), çünkü çekirdek pencere yok edildikten
//! sonra da (anlık bildirim) ve hiç pencere açılmadan da (etkinliksiz
//! çekirdek) bildirim üretir:
//!
//! - Hesap başına grup ve sabit kimlikli özet ("N yeni posta", satır satır
//!   gönderen ve konu). Aynı eşitlemede gelen postalar tek seferde gönderilir
//!   ([`NotifyQueue`]): yalnız en yenisi ses çıkarır ve üstten görünür, öteki
//!   postalar ve özet sessizdir (bir yığın = bir ses).
//! - Düğmeler kullanıcının seçtiği sırayla (`Settings.notify_actions`;
//!   varsayılan Yanıtla, Sil, İstenmeyen; en çok üç). Hesabın İstenmeyen
//!   kutusu yoksa o düğme gösterilmez ([`eylem_adlari`]).
//! - Gövdede konu ve önizlemenin ilk satırı; ek adı ve posta gövdesi taşınmaz.
//! - Kilit ekranında (kullanıcı "hassas içeriği gizle" seçtiyse) yalnız hesap
//!   adı ve "Yeni posta" görünür (`setPublicVersion`).
//! - Düğmeler `eylem.rs`te işlenir.
//!
//! Elek'e düşen postalar buraya hiç gelmez: bildirim kararı (`mh_app::sink`)
//! onları `Platform::notify`tan önce ayıklar. Okunan, taşınan ya da silinen
//! postanın bildirimi kendiliğinden kalkar ([`reconcile_soon`]); başka
//! cihazda okunan posta da buna dahildir.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use mh_core::{
    AppError, MailboxRole, MessageId, MessageSummary, Millis, NotifyAction, NotifyEvent, Settings,
};
use serde::Serialize;
use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

type Res<T> = Result<T, AppError>;

/// Yeni posta kanalı (yüksek önem; Kotlin oluşturur). Kullanıcı sistem
/// ayarlarından sesini ve önemini değiştirebilir.
pub const CHANNEL: &str = "yeni-posta";

/// Aynı eşitlemede gelen postaları toplama süresi: kısa tutulur ki tek posta
/// gecikmesin, art arda gelenler tek seste birleşsin.
const BATCH: Duration = Duration::from_millis(350);
/// Okunan/taşınan postaların bildirimlerini temizlemeden önce bekleme
/// (sayaç olayları art arda gelir; tek denetim yeter).
const RECONCILE: Duration = Duration::from_millis(1500);
/// Önizleme satırının en çok uzunluğu (karakter).
const PREVIEW_CHARS: usize = 160;

/// Tek posta bildirimi (Kotlin `ArkaKopru.bildir` → `Bildirimler.postaGoster`).
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Kart {
    pub id: MessageId,
    /// Gönderenin görünen adı (yoksa adresin yerel kısmı).
    pub gonderen: String,
    /// Konu; boşsa Kotlin "(Konu yok)" yazar.
    pub konu: String,
    /// Önizlemenin ilk dolu satırı (boş olabilir).
    pub onizleme: String,
    pub zaman: Millis,
}

/// Bir hesabın aynı yığında gelen postaları (eskiden yeniye) ve düğmeleri.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HesapGrubu {
    pub hesap: String,
    pub hesap_adi: String,
    /// Kotlin düğme adları, kullanıcının sırasıyla: `yanit`, `okundu`,
    /// `arsivle`, `sil`, `istenmeyen`.
    pub eylemler: Vec<String>,
    pub postalar: Vec<Kart>,
}

/// Kuyruktaki posta: olay ve o anki düğme listesi.
#[derive(Debug, Clone, PartialEq)]
pub struct Bekleyen {
    pub ev: NotifyEvent,
    pub eylemler: Vec<String>,
}

/// Ayardaki düğmeler Kotlin adlarıyla, sıra korunarak. İstenmeyen yalnız
/// hesabın `junk` kutusu varsa; bilinmeyen değer atlanır.
pub fn eylem_adlari(settings: &Settings, has_junk: bool) -> Vec<String> {
    settings
        .notify_actions
        .iter()
        .filter_map(|a| match a {
            NotifyAction::Reply => Some("yanit"),
            NotifyAction::MarkRead => Some("okundu"),
            NotifyAction::Archive => Some("arsivle"),
            NotifyAction::Trash => Some("sil"),
            NotifyAction::Spam if has_junk => Some("istenmeyen"),
            NotifyAction::Spam | NotifyAction::Unknown => None,
        })
        .map(str::to_owned)
        .collect()
}

/// Önizlemenin ilk dolu satırı; boşluklar teke iner, uzunsa `…` ile kısalır.
pub fn first_line(snippet: &str) -> String {
    let line = snippet
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= PREVIEW_CHARS {
        return line;
    }
    let mut cut: String = line.chars().take(PREVIEW_CHARS - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// Bildirim olayından Kotlin'e giden posta.
pub fn kart(ev: &NotifyEvent) -> Kart {
    let m = &ev.message;
    Kart {
        id: m.id,
        gonderen: m.from.label().trim().to_owned(),
        konu: m.subject.trim().to_owned(),
        onizleme: first_line(&m.snippet),
        zaman: m.date,
    }
}

/// Yığını hesaplara böler: hesaplar ilk görüldükleri sırada, postalar
/// tarihe göre eskiden yeniye; aynı posta iki kez gelirse bir kez yazılır.
/// Düğmeler hesabın son postasının listesidir (aynı ayar).
pub fn group(batch: &[Bekleyen]) -> Vec<HesapGrubu> {
    let mut out: Vec<HesapGrubu> = Vec::new();
    for b in batch {
        let ev = &b.ev;
        let account = &ev.message.account_id;
        let i = match out.iter().position(|g| &g.hesap == account) {
            Some(i) => i,
            None => {
                out.push(HesapGrubu {
                    hesap: account.clone(),
                    hesap_adi: ev.account_name.clone(),
                    eylemler: Vec::new(),
                    postalar: Vec::new(),
                });
                out.len() - 1
            }
        };
        let g = &mut out[i];
        // Hesabın güncel adı (yeniden adlandırılmış olabilir).
        g.hesap_adi = ev.account_name.clone();
        g.eylemler = b.eylemler.clone();
        match g.postalar.iter_mut().find(|k| k.id == ev.message.id) {
            Some(k) => *k = kart(ev),
            None => g.postalar.push(kart(ev)),
        }
    }
    for g in &mut out {
        g.postalar.sort_by_key(|k| (k.zaman, k.id));
    }
    out
}

/// Bildirim kuyruğu ve temizleme zamanlayıcısı; süreçte tektir, hangi
/// çekirdeğin (pencereli / etkinliksiz) ürettiğinden bağımsız.
#[derive(Default)]
pub struct NotifyQueue {
    pending: Mutex<Vec<Bekleyen>>,
    flush_scheduled: AtomicBool,
    reconcile_scheduled: AtomicBool,
}

static QUEUE: NotifyQueue = NotifyQueue {
    pending: Mutex::new(Vec::new()),
    flush_scheduled: AtomicBool::new(false),
    reconcile_scheduled: AtomicBool::new(false),
};

impl NotifyQueue {
    /// Kuyruğa ekler; yığın zamanlanmamışsa `true` döner (çağıran zamanlar).
    pub fn push(&self, b: Bekleyen) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(b);
        !self.flush_scheduled.swap(true, Ordering::AcqRel)
    }

    /// Yığını alır; bayrak önce iner: bu arada gelen posta ya bu yığına
    /// girer ya da yeni bir yığın kurar, hiçbiri kaybolmaz.
    pub fn take(&self) -> Vec<Bekleyen> {
        self.flush_scheduled.store(false, Ordering::Release);
        std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Yeni posta bildirimi: kısa bir süre toplanır, sonra tek çağrıyla
/// (hesap başına grup) gösterilir. Köprü kurulu değilse (masaüstü) sessizce
/// geçer; masaüstü ikilisi `Platform::notify`ta sistem bildirimine düşer.
pub fn show(ev: &NotifyEvent, eylemler: Vec<String>) {
    if !crate::kopru::kuruldu() {
        return;
    }
    if !QUEUE.push(Bekleyen {
        ev: ev.clone(),
        eylemler,
    }) {
        return;
    }
    let Some(rt) = crate::runtime() else {
        return flush();
    };
    rt.spawn(async {
        tokio::time::sleep(BATCH).await;
        // Köprü çağrısı Keystore'a dokunmaz ama Kotlin tarafında çalışır;
        // eşzamansız iş parçacığını tutmasın.
        let _ = tokio::task::spawn_blocking(flush).await;
    });
}

fn flush() {
    let batch = QUEUE.take();
    if batch.is_empty() {
        return;
    }
    let groups = group(&batch);
    if let Err(e) = crate::kopru::bildir(&serde_json::json!({ "gruplar": groups })) {
        tracing::warn!(error = %e, "sistem bildirimi gösterilemedi");
    }
}

/// Bildirim hâlâ yeni mi (bkz. [`is_new`]). Depo okunamazsa bildirim yerinde
/// kalır.
fn still_new(state: &mh_app::AppState, id: MessageId) -> bool {
    let role = || state.store.location(id).ok().flatten().map(|l| l.role);
    match state.store.summary(id) {
        Ok(summary) => is_new(summary.as_ref(), role),
        Err(_) => true,
    }
}

/// Okunmamış, ertelenmemiş ve gelen kutusunda; silinmiş postanın bildirimi
/// kalkar. Konumu okunamayan posta yerinde kalır.
fn is_new(summary: Option<&MessageSummary>, role: impl FnOnce() -> Option<MailboxRole>) -> bool {
    match summary {
        None => false,
        Some(s) if !s.unread || s.snoozed_until.is_some() => false,
        Some(_) => role().is_none_or(|r| r == MailboxRole::Inbox),
    }
}

/// Etkin bildirimlerden artık yeni olmayanlar.
pub fn stale(state: &mh_app::AppState, active: Vec<MessageId>) -> Vec<MessageId> {
    active
        .into_iter()
        .filter(|id| !still_new(state, *id))
        .collect()
}

/// Okunan (bu cihazda ya da başka yerde), arşivlenen ya da silinen
/// postaların bildirimlerini kaldırır. Sayaç değiştiğinde çağrılır; art arda
/// gelen çağrılar tek denetimde birleşir. Açık çekirdeğin deposuna bakar.
pub fn reconcile_soon() {
    if !crate::kopru::kuruldu() {
        return;
    }
    if QUEUE.reconcile_scheduled.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(rt) = crate::runtime() else {
        QUEUE.reconcile_scheduled.store(false, Ordering::Release);
        return;
    };
    rt.spawn(async {
        tokio::time::sleep(RECONCILE).await;
        QUEUE.reconcile_scheduled.store(false, Ordering::Release);
        let _ = tokio::task::spawn_blocking(reconcile).await;
    });
}

fn reconcile() {
    let active = match crate::kopru::etkin_bildirimler() {
        Ok(a) => a,
        Err(e) => {
            tracing::debug!(error = %e, "etkin bildirimler okunamadı");
            return;
        }
    };
    if active.is_empty() {
        return;
    }
    let Some(eski) = crate::cekirdek::with_state(|state| stale(state, active)) else {
        return;
    };
    if eski.is_empty() {
        return;
    }
    tracing::debug!(
        n = eski.len(),
        "okunan postaların bildirimleri kaldırılıyor"
    );
    if let Err(e) = crate::kopru::bildirim_kaldir(&eski) {
        tracing::warn!(error = %e, "bildirimler kaldırılamadı");
    }
}

/// İzin durumu kullanıcı ayarıyla birleşir: izin verilmiş olsa da uygulama ya
/// da posta kanalı sistem ayarlarından kapatıldıysa bildirim çıkmaz;
/// arayüz bunu da "denied" (ayarları aç) diye görür.
fn effective(state: PermissionState, enabled: bool) -> PermissionState {
    match state {
        PermissionState::Granted if !enabled => PermissionState::Denied,
        other => other,
    }
}

fn label(state: PermissionState) -> String {
    match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt => "prompt",
        PermissionState::PromptWithRationale => "promptWithRationale",
    }
    .to_owned()
}

/// Uygulama ve posta kanalı bildirimleri açık mı. Android dışında ya da
/// köprü yanıt vermezse açık sayılır (yalnız gösterge içindir).
fn enabled() -> bool {
    if !cfg!(target_os = "android") {
        return true;
    }
    match crate::kopru::bildirimler_acik(CHANNEL) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "bildirim ayarı okunamadı");
            true
        }
    }
}

fn permission_error(e: tauri_plugin_notification::Error) -> AppError {
    tracing::warn!(error = %e, "bildirim izni okunamadı");
    AppError::new(
        mh_core::ErrorCode::Internal,
        "Bildirim izni okunamadı. Sistem ayarlarından Mailhere bildirimlerini açabilirsin.",
    )
}

/// Bildirim izninin durumu; kullanıcıya sorulmaz. Sonuç: `granted` |
/// `denied` | `prompt` | `promptWithRationale`.
#[tauri::command]
pub async fn notification_status(app: AppHandle) -> Res<String> {
    let state = tokio::task::spawn_blocking(move || {
        let state = app.notification().permission_state()?;
        Ok::<_, tauri_plugin_notification::Error>(effective(state, enabled()))
    })
    .await
    .map_err(mh_app::state::internal)?
    .map_err(permission_error)?;
    Ok(label(state))
}

/// Android 13+ bildirim iznini ister (daha önce karar verildiyse sormaz);
/// sonuç `notification_status` ile aynı biçimde.
#[tauri::command]
pub async fn notification_permission(app: AppHandle) -> Res<String> {
    let state = tokio::task::spawn_blocking(move || {
        let n = app.notification();
        // İzin zaten verilmişse istek gönderilmez: eklenti o durumda yanıt
        // vermez ve çağrı sonsuza dek beklerdi.
        let state = match n.permission_state()? {
            PermissionState::Granted => PermissionState::Granted,
            _ => n.request_permission()?,
        };
        Ok::<_, tauri_plugin_notification::Error>(effective(state, enabled()))
    })
    .await
    .map_err(mh_app::state::internal)?
    .map_err(permission_error)?;
    Ok(label(state))
}

/// Mailhere'nin sistem bildirim ayarlarını açar (kalıcı retten sonra izin
/// yalnız oradan verilir). Android dışında bir şey yapmaz.
#[tauri::command]
pub async fn notification_settings(app: AppHandle) -> Res<()> {
    if !cfg!(target_os = "android") {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || {
        crate::plugin::call::<serde_json::Value>(&app, "openNotificationSettings", ())
    })
    .await
    .map_err(mh_app::state::internal)?
    .map_err(|e| {
        tracing::warn!(error = %e, "bildirim ayarları açılamadı");
        AppError::new(
            mh_core::ErrorCode::Internal,
            "Bildirim ayarları açılamadı. Telefonun Ayarlar › Uygulamalar › Mailhere bölümünden açabilirsin.",
        )
    })?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Örnek posta; çekirdeğe yeni isteğe bağlı alan eklense de derlensin
    /// diye JSON'dan kurulur.
    pub(crate) fn ozet(
        id: MessageId,
        account: &str,
        subject: &str,
        snippet: &str,
        date: Millis,
    ) -> mh_core::MessageSummary {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "accountId": account,
            "mailboxId": format!("{account}:INBOX"),
            "threadId": format!("t{id}"),
            "from": { "name": "Ayşe Demir", "email": "ayse@ornek.com" },
            "to": [],
            "subject": subject,
            "snippet": snippet,
            "date": date,
            "unread": true,
            "starred": false,
            "hasAttachments": false,
            "category": "people",
            "snoozedUntil": null,
            "followUp": null,
            "pinned": false,
            "tags": []
        }))
        .expect("örnek posta")
    }

    fn olay(id: MessageId, account: &str, name: &str, date: Millis) -> NotifyEvent {
        NotifyEvent {
            message: ozet(id, account, &format!("Konu {id}"), "", date),
            account_name: name.into(),
            account_color: "#4655D4".into(),
            otp: None,
        }
    }

    fn bekleyen(id: MessageId, account: &str, name: &str, date: Millis) -> Bekleyen {
        Bekleyen {
            ev: olay(id, account, name, date),
            eylemler: vec!["yanit".into(), "sil".into()],
        }
    }

    fn ayar(actions: Vec<NotifyAction>) -> Settings {
        let mut s: Settings = serde_json::from_value(serde_json::json!({})).expect("ayar");
        s.notify_actions = actions;
        s
    }

    #[test]
    fn varsayilan_dugmeler_yanitla_sil_istenmeyen_sirayla() {
        let s: Settings = serde_json::from_value(serde_json::json!({})).expect("ayar");
        assert_eq!(
            eylem_adlari(&s, true),
            vec!["yanit", "sil", "istenmeyen"],
            "varsayılan ayar üç düğme"
        );
        // İstenmeyen kutusu olmayan hesapta o düğme düşer, sıra korunur.
        assert_eq!(eylem_adlari(&s, false), vec!["yanit", "sil"]);
    }

    #[test]
    fn dugme_adlari_ayarin_sirasini_izler() {
        let s = ayar(vec![
            NotifyAction::Archive,
            NotifyAction::MarkRead,
            NotifyAction::Reply,
        ]);
        assert_eq!(eylem_adlari(&s, true), vec!["arsivle", "okundu", "yanit"]);
        let bos = ayar(vec![]);
        assert!(eylem_adlari(&bos, true).is_empty());
        let bilinmeyen = ayar(vec![NotifyAction::Unknown, NotifyAction::Trash]);
        assert_eq!(eylem_adlari(&bilinmeyen, true), vec!["sil"]);
    }

    #[test]
    fn kapali_kanal_izni_reddedilmis_gosterir() {
        assert_eq!(label(effective(PermissionState::Granted, true)), "granted");
        assert_eq!(label(effective(PermissionState::Granted, false)), "denied");
        assert_eq!(label(effective(PermissionState::Prompt, false)), "prompt");
        assert_eq!(
            label(effective(PermissionState::PromptWithRationale, true)),
            "promptWithRationale"
        );
        assert_eq!(label(effective(PermissionState::Denied, true)), "denied");
    }

    #[test]
    fn kart_gonderen_konu_ve_onizlemenin_ilk_satiri() {
        let ev = NotifyEvent {
            message: ozet(
                7,
                "is",
                "  Toplantı <yarın> & sonrası ",
                "\n\n  Merhaba   Ayşe,\nikinci satır",
                1_000,
            ),
            account_name: "İş".into(),
            account_color: "#000".into(),
            otp: None,
        };
        let k = kart(&ev);
        assert_eq!(k.id, 7);
        assert_eq!(k.gonderen, "Ayşe Demir");
        // Düz metin: Android işaretleme yorumlamaz, kaçışlama yapılmaz.
        assert_eq!(k.konu, "Toplantı <yarın> & sonrası");
        assert_eq!(k.onizleme, "Merhaba Ayşe,");
        assert_eq!(k.zaman, 1_000);
        assert_eq!(
            serde_json::to_value(&k).unwrap(),
            serde_json::json!({
                "id": 7,
                "gonderen": "Ayşe Demir",
                "konu": "Toplantı <yarın> & sonrası",
                "onizleme": "Merhaba Ayşe,",
                "zaman": 1000
            })
        );
    }

    #[test]
    fn adsiz_gonderen_adresin_yerel_kismi_bos_konu_bos_kalir() {
        let mut ev = olay(3, "is", "İş", 0);
        ev.message.from = mh_core::Address::new(None, "bilgi@ornek.com");
        ev.message.subject = "   ".into();
        let k = kart(&ev);
        assert_eq!(k.gonderen, "bilgi");
        assert_eq!(k.konu, "");
    }

    #[test]
    fn okunan_tasinan_ertelenen_silinen_bildirim_kalkar() {
        let fresh = ozet(1, "is", "Konu", "", 0);
        let inbox = || Some(MailboxRole::Inbox);
        assert!(is_new(Some(&fresh), inbox));
        // Konumu okunamadı: bildirim yerinde kalır.
        assert!(is_new(Some(&fresh), || None));
        assert!(!is_new(Some(&fresh), || Some(MailboxRole::Archive)));
        assert!(!is_new(Some(&fresh), || Some(MailboxRole::Trash)));
        assert!(!is_new(None, inbox));
        let mut read = fresh.clone();
        read.unread = false;
        assert!(!is_new(Some(&read), inbox));
        let mut snoozed = fresh.clone();
        snoozed.snoozed_until = Some(1);
        assert!(!is_new(Some(&snoozed), inbox));
    }

    #[test]
    fn uzun_onizleme_kisalir() {
        let long = "çok ".repeat(100);
        let line = first_line(&long);
        assert_eq!(line.chars().count(), PREVIEW_CHARS);
        assert!(line.ends_with('…'));
        assert_eq!(first_line(""), "");
        assert_eq!(first_line("  \n \t\n"), "");
    }

    #[test]
    fn yigin_hesaplara_bolunur_tekrar_yazilmaz_eskiden_yeniye() {
        let batch = vec![
            bekleyen(5, "is", "İş", 300),
            bekleyen(1, "ev", "Ev", 100),
            bekleyen(4, "is", "İş (yeni ad)", 200),
            bekleyen(5, "is", "İş (yeni ad)", 300),
        ];
        let groups = group(&batch);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].hesap, "is");
        assert_eq!(groups[0].hesap_adi, "İş (yeni ad)");
        assert_eq!(groups[0].eylemler, vec!["yanit", "sil"]);
        assert_eq!(
            groups[0].postalar.iter().map(|k| k.id).collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert_eq!(groups[1].hesap, "ev");
        assert_eq!(groups[1].postalar.len(), 1);
        let json = serde_json::to_value(&groups[1]).unwrap();
        assert_eq!(json["hesapAdi"], "Ev");
        assert_eq!(json["eylemler"], serde_json::json!(["yanit", "sil"]));
        assert!(json["postalar"].is_array());
    }

    #[test]
    fn kuyruk_ilk_eklemede_zamanlar_alinca_bosalir() {
        let q = NotifyQueue::default();
        assert!(q.push(bekleyen(1, "is", "İş", 1)));
        assert!(!q.push(bekleyen(2, "is", "İş", 2)), "yığın zaten zamanlı");
        let batch = q.take();
        assert_eq!(batch.len(), 2);
        assert!(q.take().is_empty());
        assert!(q.push(bekleyen(3, "is", "İş", 3)), "yeni yığın");
    }

    #[test]
    fn masaustunde_gosterme_sessizce_gecer() {
        // Köprü yok: kuyruğa girmez, panik yok.
        show(&olay(1, "is", "İş", 1), vec![]);
        reconcile_soon();
    }
}
