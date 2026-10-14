//! Android kabuğunun `Platform` uygulaması: sistem bildirimi, tek pencere
//! (hızlı okuma ve yazma aynı pencerede açılır), sistemin dosya seçici ve
//! kaydet penceresi, `FileProvider` ile açma / paylaşma, açılış hatası.

use std::path::{Path, PathBuf};

use mh_app::routes::compose_route;
use mh_app::state::internal;
use mh_app::{BoxFut, Confirm, Platform};
use mh_core::{
    events, AppError, AppResult, ComposePrefill, MessageId, NavigateEvent, NotifyEvent, OpenEvent,
    Settings,
};
use tauri::{AppHandle, Emitter, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

pub struct AndroidPlatform;

impl Platform for AndroidPlatform {
    fn notify(&self, app: &AppHandle, ev: NotifyEvent, _settings: &Settings) {
        crate::notify::show(app, &ev);
    }

    /// Android'de tepsi ya da rozet yok; sayacı arayüz `mh:changed` ile yeniler.
    fn counts_changed(&self, _app: &AppHandle) {}

    /// Kart penceresi yok; sessize alma bildirim kararında uygulanır.
    fn muted(&self, _app: &AppHandle) {}

    fn open_main(&self, app: &AppHandle, id: Option<MessageId>) {
        if let Some(id) = id {
            if let Err(e) = app.emit_to(crate::MAIN, events::OPEN, OpenEvent { id }) {
                tracing::warn!(error = %e, "açma olayı gönderilemedi");
            }
        }
    }

    fn open_quick(&self, app: &AppHandle, id: MessageId) -> AppResult<()> {
        self.open_main(app, Some(id));
        Ok(())
    }

    /// Ayrı yazma penceresi yoktur: ana pencere `mh:navigate` olayıyla rotayı
    /// yığına iter (`mh_core::events::NAVIGATE`, `NavigateEvent`).
    fn open_compose(&self, app: &AppHandle, prefill: &ComposePrefill) -> AppResult<()> {
        let route = compose_route(prefill);
        app.emit_to(crate::MAIN, events::NAVIGATE, NavigateEvent { route })
            .map_err(internal)
    }

    /// Güvenli türler `ACTION_VIEW` ile uygun uygulamada açılır; diğerleri
    /// (betik, paket vb.) yalnız paylaşım sayfasına verilir, kullanıcı hangi
    /// uygulamanın alacağını bilerek seçer.
    fn open_file(&self, app: &AppHandle, path: &Path, mime: &str, direct: bool) -> AppResult<()> {
        if direct {
            crate::files::open(app, path, mime)
        } else {
            crate::files::share(app, path, mime)
        }
    }

    fn save_file<'a>(
        &'a self,
        window: &'a WebviewWindow,
        suggested: &'a str,
        bytes: Vec<u8>,
    ) -> BoxFut<'a, AppResult<Option<String>>> {
        Box::pin(crate::files::save(window, suggested, bytes))
    }

    fn pick_files<'a>(&'a self, window: &'a WebviewWindow) -> BoxFut<'a, AppResult<Vec<PathBuf>>> {
        Box::pin(crate::files::pick(window))
    }

    fn confirm<'a>(
        &'a self,
        window: &'a WebviewWindow,
        req: Confirm,
    ) -> BoxFut<'a, AppResult<bool>> {
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            // Android'de iletişim kutusunun ayrı bir ebeveyni yoktur (tek etkinlik).
            window
                .dialog()
                .message(req.message)
                .title(req.title)
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    req.ok.into(),
                    req.cancel.into(),
                ))
                .show(move |ok| {
                    let _ = tx.send(ok);
                });
            rx.await.map_err(internal)
        })
    }

    /// Açılışta veritabanı ya da kasa açılamadı: pencere açılmadan sorunu ve
    /// çözümü anlatan bir iletişim kutusu gösterilir, kapatılınca uygulama
    /// çıkar. Hiçbir dosya silinmez.
    fn fatal(&self, app: &AppHandle, e: &AppError) {
        let exit = app.clone();
        app.dialog()
            .message(e.message.clone())
            .title("Mailhere açılamadı")
            .kind(MessageDialogKind::Error)
            .buttons(MessageDialogButtons::OkCustom("Kapat".into()))
            .show(move |_| exit.exit(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yazma_olayi_rota_tasir() {
        let prefill = ComposePrefill {
            mode: mh_core::ComposeMode::Reply,
            message_id: Some(7),
            to: Vec::new(),
            account_id: None,
        };
        let ev = NavigateEvent {
            route: compose_route(&prefill),
        };
        assert_eq!(
            serde_json::to_value(&ev).unwrap(),
            serde_json::json!({ "route": "compose/reply/7" })
        );
        assert_eq!(events::NAVIGATE, "mh:navigate");
    }
}
