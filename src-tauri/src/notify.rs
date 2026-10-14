//! Sistem bildirimi: kanal `yeni-posta` (yüksek önem), Android 13+ izin
//! isteği, bildirime dokununca postanın açılması. Metin gönderen ve konudan
//! ibarettir; gövde ve ek adı taşınmaz.

use mh_core::{AppError, NotifyEvent};
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

/// Android 13+ bildirim iznini ister (daha önce karar verildiyse sormaz);
/// sonuç: `granted` | `denied` | `prompt` | `promptWithRationale`.
#[tauri::command]
pub async fn notification_permission(app: AppHandle) -> Res<String> {
    use tauri::plugin::PermissionState;
    let state = tokio::task::spawn_blocking(move || app.notification().request_permission())
        .await
        .map_err(mh_app::state::internal)?
        .map_err(|e| {
            tracing::warn!(error = %e, "bildirim izni istenemedi");
            AppError::new(
                mh_core::ErrorCode::Internal,
                "Bildirim izni istenemedi. Sistem ayarlarından Mailhere bildirimlerini açabilirsin.",
            )
        })?;
    Ok(match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        PermissionState::Prompt => "prompt",
        PermissionState::PromptWithRationale => "promptWithRationale",
    }
    .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
