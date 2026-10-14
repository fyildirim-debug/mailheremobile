//! Sistem bildirimi: kanal `yeni-posta` (yüksek önem), Android 13+ izin
//! isteği, bildirime dokununca postanın açılması. Metin gönderen ve konudan
//! ibarettir; gövde ve ek adı taşınmaz.

use mh_core::{AppError, NotifyEvent};
use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

type Res<T> = Result<T, AppError>;

/// Bildirim kanalı; kullanıcı sistem ayarlarından sesini ve önemini değiştirebilir.
pub const CHANNEL: &str = "yeni-posta";
/// Bildirimin `extra` alanındaki posta kimliği (dokununca açmak için).
const EXTRA_ID: &str = "id";

/// Bildirim başlığı ve gövdesi (düz metin; Android işaretleme yorumlamaz).
pub fn texts(ev: &NotifyEvent) -> (String, String) {
    let title = format!("{} · {}", ev.message.from.label(), ev.account_name);
    let body = if ev.message.subject.trim().is_empty() {
        "(Konu yok)".to_owned()
    } else {
        ev.message.subject.clone()
    };
    (title, body)
}

/// Kanalı oluşturur ve dokunma dinleyicisini bağlar (yalnız Android).
pub fn setup(app: &AppHandle) {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_notification::{Channel, Importance};
        let channel = Channel::builder(CHANNEL, "Yeni posta")
            .description("Kişilerden gelen yeni postalar")
            .importance(Importance::High)
            .build();
        if let Err(e) = app.notification().create_channel(channel) {
            tracing::warn!(error = %e, "bildirim kanalı oluşturulamadı");
        }
        let handle = app.clone();
        let res = app.notification().on_action(move |action| {
            let id = action
                .notification()
                .and_then(|n| n.extra().get(EXTRA_ID))
                .and_then(|v| v.as_i64());
            if let Some(id) = id {
                tracing::debug!(id, action = action.action_id(), "bildirime dokunuldu");
                if let Some(state) = tauri::Manager::try_state::<mh_app::AppState>(&handle) {
                    state.platform.open_main(&handle, Some(id));
                }
            }
        });
        if let Err(e) = res {
            tracing::warn!(error = %e, "bildirim dinleyicisi bağlanamadı");
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
    }
}

/// Yeni posta bildirimi.
pub fn show(app: &AppHandle, ev: &NotifyEvent) {
    let (title, body) = texts(ev);
    let res = app
        .notification()
        .builder()
        .channel_id(CHANNEL)
        .title(title)
        .body(body)
        .extra(EXTRA_ID, ev.message.id)
        .auto_cancel()
        .show();
    if let Err(e) = res {
        tracing::warn!(error = %e, "sistem bildirimi gösterilemedi");
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
/// eklenti yanıt vermezse açık sayılır (yalnız gösterge içindir).
fn enabled(app: &AppHandle) -> bool {
    #[derive(serde::Deserialize)]
    struct Enabled {
        enabled: bool,
    }
    if !cfg!(target_os = "android") {
        return true;
    }
    match crate::plugin::call::<Enabled>(
        app,
        "notificationsEnabled",
        serde_json::json!({ "channel": CHANNEL }),
    ) {
        Ok(e) => e.enabled,
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
        Ok::<_, tauri_plugin_notification::Error>(effective(state, enabled(&app)))
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
        Ok::<_, tauri_plugin_notification::Error>(effective(state, enabled(&app)))
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
mod tests {
    use super::*;

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
    fn bildirim_metni_gonderen_ve_konu() {
        let summary = mh_core::MessageSummary {
            id: 7,
            account_id: "is".into(),
            mailbox_id: "INBOX".into(),
            thread_id: "t1".into(),
            from: mh_core::Address::new(Some("Ayşe Demir"), "ayse@ornek.com"),
            to: Vec::new(),
            subject: "Toplantı <yarın> & sonrası".into(),
            snippet: "gizli gövde".into(),
            date: 0,
            unread: true,
            starred: false,
            has_attachments: true,
            category: mh_core::Category::People,
            snoozed_until: None,
            follow_up: None,
            pinned: false,
            tags: Vec::new(),
        };
        let ev = NotifyEvent {
            message: summary,
            account_name: "İş".into(),
            account_color: "#000".into(),
        };
        let (title, body) = texts(&ev);
        assert_eq!(title, "Ayşe Demir · İş");
        // Düz metin: Android işaretleme yorumlamaz, kaçışlama yapılmaz.
        assert_eq!(body, "Toplantı <yarın> & sonrası");
        let mut empty = ev.clone();
        empty.message.subject = "  ".into();
        assert_eq!(texts(&empty).1, "(Konu yok)");
    }
}
