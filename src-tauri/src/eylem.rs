//! Bildirimden gelen eylemler: posta açma, Okundu, Arşivle, satır içi Yanıtla
//! ve yanıtı geri alma. Uygulama ekranı açılmadan çekirdekte çalışır.
//!
//! Yol: bildirim düğmesi → `BildirimAlici` (Kotlin, `exported=false`) →
//! `MhPlugin.trigger("bildirim", …)` → burada kayıtlı kanal → `mh-app`
//! işlevleri (`messages_flag`, `messages_move`, giden kutusu). İşlemler önce
//! yerelde uygulanır, sunucuya kalıcı işlem kuyruğuyla gider: ağ yoksa
//! bağlantı gelince uygulanır. Bitince bildirim kalkar ve özet güncellenir;
//! olmazsa bildirimde "ne oldu + ne yapılabilir" yazar.
//!
//! Süreç ölmüşken basılan düğme çekirdeği başlatamaz (Rust çalışma ortamı
//! yalnız etkinlikle açılır; bkz. `docs/ANDROID.md` "Arka plan"). Kotlin
//! eylemi şifreli kasaya (`bildirim-bekleyen`) yazar, bildirimi "Mailhere
//! açılınca uygulanacak" durumuna çevirir; çekirdek açılınca [`setup`]
//! kuyruğu boşaltıp aynı yoldan uygular.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use chrono::TimeZone;
use mh_core::{
    Account, Address, AppError, AppResult, ErrorCode, Language, MessageDetail, MessageId, Millis,
    MoveTarget, OutboxItem, OutgoingMessage,
};
use serde::Deserialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager};

use crate::notify::plugin_call;

/// Kotlin'den gelen olayın adı (`MhPlugin.OLAY`).
pub const EVENT: &str = "bildirim";
/// Yanıtlanacak postanın gövdesi inmemişse en çok bu kadar beklenir; sonra
/// önizleme alıntılanır.
const BODY_WAIT: Duration = Duration::from_secs(8);
/// Soğuk açılışta arayüzün olay dinleyicileri sayfa yüklendikten sonra
/// kurulur; bildirimden açılan posta bu kadar bekletilir.
const UI_SETTLE: Duration = Duration::from_millis(1200);

/// Bildirimden gelen eylem.
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

#[derive(Deserialize, Default)]
struct Bekleyen {
    #[serde(default)]
    eylemler: Vec<serde_json::Value>,
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

/// Kotlin'den gelen olayları dinler ve süreç kapalıyken biriken eylemleri
/// uygular. `setup_core`dan sonra, ana pencere açılmadan çağrılır.
pub fn setup(app: &AppHandle) {
    app.manage(UiGate::default());
    if !cfg!(target_os = "android") {
        return;
    }
    let handle = app.clone();
    let channel: Channel = Channel::new(move |body| {
        let parsed = match body {
            InvokeResponseBody::Json(s) => {
                serde_json::from_str::<Eylem>(&s).map_err(|e| e.to_string())
            }
            InvokeResponseBody::Raw(_) => Err("ham veri".to_owned()),
        };
        match parsed {
            Ok(e) => dispatch(&handle, e),
            Err(e) => tracing::warn!(error = %e, "bildirim eylemi okunamadı"),
        }
        Ok(())
    });
    if let Err(e) = crate::plugin::call::<serde_json::Value>(
        app,
        "registerListener",
        serde_json::json!({ "event": EVENT, "handler": channel }),
    ) {
        tracing::warn!(error = %e, "bildirim eylemleri dinlenemiyor");
        return;
    }
    // Süreç kapalıyken basılan düğmeler ve soğuk açılıştaki dokunuş.
    match crate::plugin::call::<Bekleyen>(app, "bekleyenEylemler", serde_json::json!({})) {
        Ok(b) => {
            for v in b.eylemler {
                match serde_json::from_value::<Eylem>(v) {
                    Ok(e) => dispatch(app, e),
                    Err(e) => tracing::warn!(error = %e, "bekleyen eylem okunamadı"),
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "bekleyen bildirim eylemleri okunamadı"),
    }
}

/// Eylemi çalışma ortamına verir (Kotlin'in ana iş parçacığını tutmaz).
fn dispatch(app: &AppHandle, e: Eylem) {
    tracing::debug!(eylem = ?kind(&e), "bildirim eylemi");
    if let Eylem::Ac { id } = e {
        return open_when_ready(app, id);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(&app, e).await });
}

fn kind(e: &Eylem) -> &'static str {
    match e {
        Eylem::Ac { .. } => "ac",
        Eylem::Okundu { .. } => "okundu",
        Eylem::Arsivle { .. } => "arsivle",
        Eylem::Yanit { .. } => "yanit",
        Eylem::GeriAl { .. } => "geriAl",
        Eylem::Uyan => "uyan",
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(mh_app::state::internal)?
}

async fn run(app: &AppHandle, e: Eylem) {
    match e {
        Eylem::Ac { id } => open_when_ready(app, id),
        Eylem::Okundu { id } => {
            let h = app.clone();
            let res = blocking(move || {
                mh_app::commands::messages_flag(h.clone(), h.state(), vec![id], Some(false), None)
            })
            .await;
            done(app, id, res).await;
        }
        Eylem::Arsivle { id } => {
            let h = app.clone();
            let res = blocking(move || {
                mh_app::commands::messages_move(h.clone(), h.state(), vec![id], MoveTarget::Archive)
            })
            .await;
            done(app, id, res).await;
        }
        Eylem::Yanit { id, metin } => match reply(app, id, &metin).await {
            Ok(item) => {
                // Yanıtlanan posta okunmuş sayılır; olmazsa yanıt yine gider.
                let h = app.clone();
                let _ = blocking(move || {
                    mh_app::commands::messages_flag(
                        h.clone(),
                        h.state(),
                        vec![id],
                        Some(false),
                        None,
                    )
                })
                .await;
                let res = plugin_call::<serde_json::Value>(
                    app,
                    "yanitKuyrukta",
                    serde_json::json!({
                        "id": id,
                        "kuyruk": item.id,
                        "gonderim": item.send_at,
                        "metin": metin.trim(),
                    }),
                )
                .await;
                if let Err(e) = res {
                    tracing::warn!(error = %e, "yanıt bildirimi güncellenemedi");
                }
            }
            Err(e) => failed(app, id, &e).await,
        },
        Eylem::GeriAl { id, kuyruk } => {
            let Some(state) = app.try_state::<mh_app::AppState>() else {
                return;
            };
            let outbox = state.outbox.clone();
            match blocking(move || outbox.cancel(&kuyruk)).await {
                Ok(iptal) => {
                    let res = plugin_call::<serde_json::Value>(
                        app,
                        "yanitGeriAlindi",
                        serde_json::json!({ "id": id, "iptal": iptal }),
                    )
                    .await;
                    if let Err(e) = res {
                        tracing::warn!(error = %e, "yanıt bildirimi güncellenemedi");
                    }
                }
                Err(e) => failed(app, id, &e).await,
            }
        }
        Eylem::Uyan => {
            // Uykuda yarı ölü kalmış bağlantılar bir eşitlemeyle sınanır;
            // kopmuşsa motor yeniden bağlanır, yeni posta bildirilir.
            if let Some(state) = app.try_state::<mh_app::AppState>() {
                state.sync.sync_now();
                state.jmap.sync_now();
            }
        }
    }
}

/// Okundu / Arşivle sonucu: başarıda bildirim kalkar, hatada nedeni yazar.
async fn done(app: &AppHandle, id: MessageId, res: AppResult<()>) {
    match res {
        Ok(()) => {
            if let Err(e) = plugin_call::<serde_json::Value>(
                app,
                "bildirimKaldir",
                serde_json::json!({ "ids": [id] }),
            )
            .await
            {
                tracing::warn!(error = %e, "bildirim kaldırılamadı");
            }
        }
        Err(e) => failed(app, id, &e).await,
    }
}

async fn failed(app: &AppHandle, id: MessageId, e: &AppError) {
    tracing::warn!(code = %e.code, error = %e.message, "bildirim eylemi uygulanamadı");
    let res = plugin_call::<serde_json::Value>(
        app,
        "bildirimHata",
        serde_json::json!({ "id": id, "metin": e.message }),
    )
    .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, "bildirim güncellenemedi");
    }
}

/// Yanıtı giden kutusuna koyar; gönderim, kullanıcının "gönderimi geri al"
/// süresi dolunca olur.
async fn reply(app: &AppHandle, id: MessageId, text: &str) -> AppResult<OutboxItem> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return Err(AppError::new(
            ErrorCode::Invalid,
            "Yanıt boş. Bildirimden yeniden yaz ya da postayı uygulamada aç.",
        ));
    }
    let state = app.try_state::<mh_app::AppState>().ok_or_else(|| {
        AppError::new(
            ErrorCode::Internal,
            "Mailhere henüz hazır değil; birazdan yeniden dene.",
        )
    })?;
    // Gövde inmemişse kısa süre beklenir (ağ yoksa önizleme alıntılanır).
    let detail =
        match tokio::time::timeout(BODY_WAIT, mh_app::commands::message_get(app.state(), id)).await
        {
            Ok(Ok(d)) => d,
            _ => {
                let store = state.store.clone();
                blocking(move || {
                    let allow = store.settings()?.remote_allow;
                    store.detail(id, &allow)
                })
                .await?
            }
        };
    let store = state.store.clone();
    let account_id = detail.summary.account_id.clone();
    let (account, lang) = blocking(move || {
        let account = store
            .account(&account_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Bu postanın hesabı artık yok."))?;
        Ok((account, store.settings()?.language))
    })
    .await?;
    let input = ReplyInput::from_detail(&detail);
    let msg = reply_message(&input, &account, &text, lang, &format_date(input.date));
    let outbox = state.outbox.clone();
    blocking(move || outbox.enqueue(&msg, mh_app::state::now_ms())).await
}

/// Yanıt için gereken alanlar (çekirdeğin `MessageDetail`inden).
#[derive(Debug, Clone, PartialEq)]
pub struct ReplyInput {
    pub id: MessageId,
    pub account_id: String,
    pub from: Address,
    pub reply_to: Vec<Address>,
    pub to: Vec<Address>,
    pub subject: String,
    /// Alıntılanacak metin: gövde, yoksa önizleme.
    pub quote: String,
    pub date: Millis,
}

impl ReplyInput {
    pub fn from_detail(d: &MessageDetail) -> Self {
        let s = &d.summary;
        Self {
            id: s.id,
            account_id: s.account_id.clone(),
            from: s.from.clone(),
            reply_to: d.reply_to.clone(),
            to: s.to.clone(),
            subject: s.subject.clone(),
            quote: d
                .body_text
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| s.snippet.clone()),
            date: s.date,
        }
    }
}

fn format_date(ms: Millis) -> String {
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|d| d.format("%d.%m.%Y %H:%M").to_string())
        .unwrap_or_default()
}

/// Konudan yanıt/iletme öneklerini atar (`Re:`, `Fwd:`, `Ynt:`, `İlt:`,
/// `Aw:`, `Wg:`, `Cevap:`; `Re[2]:` gibi sayılı biçimler dahil).
pub fn base_subject(subject: &str) -> String {
    const PREFIXES: [&str; 8] = ["re", "fw", "fwd", "ynt", "ilt", "cevap", "aw", "wg"];
    let mut s = subject.trim();
    while let Some(colon) = s.find(':') {
        let head = s[..colon].trim();
        // `Re[2]` → `re`
        let head = match head.find('[') {
            Some(b)
                if head.ends_with(']')
                    && head[b + 1..head.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit()) =>
            {
                head[..b].trim_end()
            }
            _ => head,
        };
        let folded: String = head
            .chars()
            .map(|c| match c {
                'İ' | 'I' | 'ı' => 'i',
                c => c.to_ascii_lowercase(),
            })
            .collect();
        if !PREFIXES.contains(&folded.as_str()) {
            break;
        }
        s = s[colon + 1..].trim_start();
    }
    s.trim().to_owned()
}

/// Yanıtın alıcıları: Reply-To ya da gönderen, hesabın kendi adresi
/// çıkarılmış; kimse kalmazsa (kendi gönderdiğimiz posta) postanın
/// alıcıları; o da boşsa yanıt adresleri. Arayüzdeki hızlı yanıtla aynı kural.
pub fn reply_targets(input: &ReplyInput, own_email: &str) -> Vec<Address> {
    let me = own_email.trim().to_lowercase();
    let not_me = |a: &&Address| a.email.trim().to_lowercase() != me;
    let targets: Vec<Address> = if input.reply_to.is_empty() {
        vec![input.from.clone()]
    } else {
        input.reply_to.clone()
    };
    let others: Vec<Address> = targets.iter().filter(not_me).cloned().collect();
    if !others.is_empty() {
        return others;
    }
    let to: Vec<Address> = input.to.iter().filter(not_me).cloned().collect();
    if to.is_empty() {
        targets
    } else {
        to
    }
}

/// Yanıt postası: metin (+ hesabın yanıt imzası) ve alıntı, arayüzün hızlı
/// yanıtıyla aynı biçimde. Ek yok; gönderim zamanı giden kutusunda belirlenir.
pub fn reply_message(
    input: &ReplyInput,
    account: &Account,
    text: &str,
    lang: Language,
    date: &str,
) -> OutgoingMessage {
    let signature = account
        .signature_reply
        .as_ref()
        .and_then(|id| account.signatures.iter().find(|s| &s.id == id))
        .map(|s| s.body.trim_end())
        .filter(|b| !b.trim().is_empty());
    let head = match signature {
        Some(sig) => format!("{}\n\n{sig}", text.trim()),
        None => text.trim().to_owned(),
    };
    let name = input.from.label();
    let wrote = match lang {
        Language::Tr => format!("{date} tarihinde {name} şunu yazdı:"),
        Language::En => format!("On {date}, {name} wrote:"),
    };
    let quoted = input
        .quote
        .split('\n')
        .map(|l| format!("> {}", l.trim_end_matches('\r')))
        .collect::<Vec<_>>()
        .join("\n");
    let subject = format!("Re: {}", base_subject(&input.subject));
    // Çekirdeğe yeni isteğe bağlı alan eklense de derlensin diye JSON'dan
    // kurulur (alanlar `serde(default)`).
    serde_json::from_value(serde_json::json!({
        "accountId": input.account_id,
        "to": reply_targets(input, &account.email),
        "cc": [],
        "bcc": [],
        "subject": subject.trim_end(),
        "bodyText": format!("{}\n\n{wrote}\n{quoted}\n", head.trim()),
        "inReplyTo": input.id,
        "attachments": [],
        "sendAt": null
    }))
    .expect("yanıt postası alanları")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Address {
        Address::new(name, email)
    }

    fn input() -> ReplyInput {
        ReplyInput {
            id: 42,
            account_id: "is".into(),
            from: addr(Some("Ayşe Demir"), "ayse@ornek.com"),
            reply_to: vec![],
            to: vec![addr(None, "ben@ornek.com")],
            subject: "Re: Ynt: Toplantı".into(),
            quote: "Merhaba,\r\nyarın 10'da?".into(),
            date: 0,
        }
    }

    fn account() -> Account {
        serde_json::from_value(serde_json::json!({
            "id": "is",
            "name": "İş",
            "email": "Ben@Ornek.com",
            "displayName": "Ben",
            "color": "#4655D4",
            "kind": "imap",
            "imap": null,
            "smtp": null,
            "jmapUrl": null,
            "signatures": [{ "id": "s1", "name": "Kısa", "body": "Sevgiler,\nBen\n\n" }],
            "signatureNew": null,
            "signatureReply": "s1",
            "createdAt": 0
        }))
        .expect("örnek hesap")
    }

    #[test]
    fn eylemler_kotlin_bicimini_okur() {
        let e: Eylem = serde_json::from_str(r#"{"tur":"okundu","id":5,"hesap":"is"}"#).unwrap();
        assert_eq!(e, Eylem::Okundu { id: 5 });
        let e: Eylem = serde_json::from_str(r#"{"tur":"arsivle","id":6}"#).unwrap();
        assert_eq!(e, Eylem::Arsivle { id: 6 });
        let e: Eylem = serde_json::from_str(r#"{"tur":"yanit","id":7,"metin":"Tamam"}"#).unwrap();
        assert_eq!(
            e,
            Eylem::Yanit {
                id: 7,
                metin: "Tamam".into()
            }
        );
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
        let e: Eylem = serde_json::from_str(r#"{"tur":"uyan"}"#).unwrap();
        assert_eq!(e, Eylem::Uyan);
        assert!(serde_json::from_str::<Eylem>(r#"{"tur":"sil","id":1}"#).is_err());
    }

    #[test]
    fn konu_onekleri_atilir() {
        assert_eq!(base_subject("Re: Ynt: Toplantı"), "Toplantı");
        assert_eq!(base_subject("RE[2]: fwd:  Rapor "), "Rapor");
        assert_eq!(base_subject("İLT: Cevap: AW: WG: Fw: Tatil"), "Tatil");
        assert_eq!(base_subject("Not: bugün"), "Not: bugün");
        assert_eq!(base_subject("Saat 10:30"), "Saat 10:30");
        assert_eq!(base_subject(""), "");
    }

    #[test]
    fn alicilar_kendi_adresim_cikarilir() {
        let mut i = input();
        assert_eq!(
            reply_targets(&i, "ben@ornek.com"),
            vec![addr(Some("Ayşe Demir"), "ayse@ornek.com")]
        );
        // Reply-To önceliklidir.
        i.reply_to = vec![addr(None, "liste@ornek.com")];
        assert_eq!(
            reply_targets(&i, "ben@ornek.com"),
            vec![addr(None, "liste@ornek.com")]
        );
        // Kendi gönderdiğim posta: alıcılara gider.
        let mut own = input();
        own.from = addr(Some("Ben"), "BEN@ornek.com");
        own.to = vec![addr(None, "ben@ornek.com"), addr(None, "ali@ornek.com")];
        assert_eq!(
            reply_targets(&own, "ben@ornek.com"),
            vec![addr(None, "ali@ornek.com")]
        );
        // Kimse kalmazsa yanıt adresleri.
        own.to = vec![];
        assert_eq!(
            reply_targets(&own, "ben@ornek.com"),
            vec![addr(Some("Ben"), "BEN@ornek.com")]
        );
    }

    #[test]
    fn yanit_metni_imza_ve_alinti() {
        let msg = reply_message(
            &input(),
            &account(),
            "  Olur, görüşürüz. ",
            Language::Tr,
            "15.10.2026 09:30",
        );
        assert_eq!(msg.account_id, "is");
        assert_eq!(msg.subject, "Re: Toplantı");
        assert_eq!(msg.in_reply_to, Some(42));
        assert_eq!(msg.to, vec![addr(Some("Ayşe Demir"), "ayse@ornek.com")]);
        assert!(msg.cc.is_empty() && msg.bcc.is_empty() && msg.attachments.is_empty());
        assert_eq!(msg.send_at, None);
        assert_eq!(
            msg.body_text,
            "Olur, görüşürüz.\n\nSevgiler,\nBen\n\n15.10.2026 09:30 tarihinde Ayşe Demir şunu yazdı:\n> Merhaba,\n> yarın 10'da?\n"
        );
        let mut a = account();
        a.signature_reply = None;
        let en = reply_message(&input(), &a, "OK", Language::En, "15.10.2026 09:30");
        assert_eq!(
            en.body_text,
            "OK\n\nOn 15.10.2026 09:30, Ayşe Demir wrote:\n> Merhaba,\n> yarın 10'da?\n"
        );
    }
}
