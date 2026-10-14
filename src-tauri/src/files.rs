//! Dosyalar: sistemin dosya seçicisi (ek ekleme), kaydet penceresi (SAF),
//! `FileProvider` ile açma ve paylaşma; Android'e özgü `attachment_share` ve
//! `attachment_save` komutları.
//!
//! Seçilen dosya `content://` adresidir: içeriği Rust tarafında okunur ve
//! uygulamanın önbelleğine (`cache/giden/<uuid>/<ad>`) kopyalanır; postaya
//! yalnız o kopya eklenir. Uygulama başka uygulamaların dosyalarını yol
//! vererek okuyamaz. Açma ve paylaşma yalnız `cache/ekler/` altındaki
//! dosyalar için, o dosyaya özel geçici okuma izniyle yapılır.

use std::path::{Path, PathBuf};

use mh_app::commands::{ensure_body, private_dir, sanitize_filename, write_private};
use mh_app::state::internal;
use mh_app::AppState;
use mh_core::{AppError, AppResult, ErrorCode, MessageId};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, AppError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileArgs<'a> {
    path: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
struct UriArg<'a> {
    uri: &'a str,
}

#[derive(Deserialize)]
struct NameResponse {
    name: Option<String>,
}

fn open_err(e: impl std::fmt::Display) -> AppError {
    tracing::warn!(error = %e, "dosya açılamadı");
    AppError::new(ErrorCode::Internal, "Dosya açılamadı.")
}

/// Dosyayı türüne uygun uygulamada açar (`ACTION_VIEW`, FileProvider).
pub fn open(app: &AppHandle, path: &Path, mime: &str) -> AppResult<()> {
    if cfg!(target_os = "android") {
        crate::plugin::call::<serde_json::Value>(
            app,
            "openFile",
            FileArgs {
                path: &path.to_string_lossy(),
                mime,
            },
        )
        .map(|_| ())
        .map_err(open_err)
    } else {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(open_err)
    }
}

/// Dosyayı paylaşım sayfasına verir (`ACTION_SEND`, FileProvider).
pub fn share(app: &AppHandle, path: &Path, mime: &str) -> AppResult<()> {
    crate::plugin::call::<serde_json::Value>(
        app,
        "shareFile",
        FileArgs {
            path: &path.to_string_lossy(),
            mime,
        },
    )
    .map(|_| ())
    .map_err(|e| {
        tracing::warn!(error = %e, "dosya paylaşılamadı");
        AppError::new(ErrorCode::Internal, "Dosya paylaşılamadı.")
    })
}

/// `content://` adresinin görünen adı (Kotlin `fileName`); bulunamazsa
/// adresin son parçası, o da yoksa `ek`.
fn display_name(app: &AppHandle, uri: &tauri::Url) -> String {
    let from_plugin =
        crate::plugin::call::<NameResponse>(app, "fileName", UriArg { uri: uri.as_str() })
            .ok()
            .and_then(|r| r.name)
            .filter(|n| !n.trim().is_empty());
    let raw = from_plugin
        .or_else(|| {
            uri.path_segments()
                .and_then(|mut s| s.next_back())
                .map(percent_decode)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "ek".to_owned());
    sanitize_filename(&raw)
}

/// Yalnız `%XX` çözümü (adres parçası için yeterli).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Seçilen dosyanın içeriğini önbelleğe kopyalar; kopyanın yolu döner.
fn copy_picked(
    app: &AppHandle,
    cache: &Path,
    file: tauri_plugin_fs::FilePath,
) -> AppResult<PathBuf> {
    use std::io::Read;
    use tauri_plugin_fs::{FsExt, OpenOptions};

    let name = match &file {
        tauri_plugin_fs::FilePath::Url(u) => display_name(app, u),
        tauri_plugin_fs::FilePath::Path(p) => p
            .file_name()
            .map(|n| sanitize_filename(&n.to_string_lossy()))
            .unwrap_or_else(|| "ek".to_owned()),
    };
    let dir = cache.join("giden").join(uuid::Uuid::new_v4().to_string());
    private_dir(&dir).map_err(internal)?;
    let mut opts = OpenOptions::new();
    opts.read(true);
    let mut src = app.fs().open(file, opts).map_err(|e| {
        tracing::warn!(error = %e, "seçilen dosya açılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Seçilen dosya okunamadı. Dosyayı yeniden seç.",
        )
    })?;
    let mut bytes = Vec::new();
    src.read_to_end(&mut bytes).map_err(|e| {
        tracing::warn!(error = %e, "seçilen dosya okunamadı");
        AppError::new(
            ErrorCode::Storage,
            "Seçilen dosya okunamadı. Dosyayı yeniden seç.",
        )
    })?;
    let path = dir.join(name);
    write_private(&path, &bytes).map_err(|e| {
        tracing::warn!(error = %e, "ek kopyalanamadı");
        AppError::new(
            ErrorCode::Storage,
            "Ek önbelleğe kopyalanamadı. Boş alanı denetle.",
        )
    })?;
    Ok(path)
}

/// Dosya seçici; seçilenler önbelleğe kopyalanır ve kopyaların yolları döner
/// (klasör ve aygıt süzmesi ortak katmanda, `files::accept`).
pub async fn pick(window: &WebviewWindow) -> AppResult<Vec<PathBuf>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window.dialog().file().pick_files(move |files| {
        let _ = tx.send(files);
    });
    let files = rx.await.map_err(internal)?.unwrap_or_default();
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let app = window.app_handle().clone();
    let cache = app.state::<AppState>().cache_dir.clone();
    // Okuma ve kopyalama engelleyicidir (içerik sağlayıcı, disk).
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for file in files {
            match copy_picked(&app, &cache, file) {
                Ok(p) => out.push(p),
                Err(e) => tracing::warn!(error = %e.message, "seçilen dosya atlandı"),
            }
        }
        Ok(out)
    })
    .await
    .map_err(internal)?
}

/// Kaydet penceresindeki süzgeç adı.
fn filter_label(ext: &str) -> String {
    match ext {
        "eml" => "E-posta (.eml)".to_owned(),
        other => format!(".{other}"),
    }
}

/// Kaydet penceresi (Android'de sistemin belge oluşturucusu): kullanıcının
/// seçtiği `content://` adresine yazılır, adres döner; vazgeçilirse `None`.
/// Masaüstünde (geliştirme) dosya yoluna 0600 ile yazılır.
pub async fn save(
    window: &WebviewWindow,
    suggested: &str,
    bytes: Vec<u8>,
) -> AppResult<Option<String>> {
    let ext = Path::new(suggested)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = window.dialog().file().set_file_name(suggested);
    if !ext.is_empty() {
        dialog = dialog.add_filter(filter_label(&ext), &[ext.as_str()]);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(picked) = rx.await.map_err(internal)? else {
        return Ok(None);
    };
    let app = window.app_handle().clone();
    tokio::task::spawn_blocking(move || write_picked(&app, picked, &ext, bytes))
        .await
        .map_err(internal)?
}

fn write_err(e: impl std::fmt::Display) -> AppError {
    tracing::warn!(error = %e, "dosya yazılamadı");
    AppError::new(
        ErrorCode::Storage,
        "Dosya yazılamadı. Seçilen konumun yazılabilir olduğunu ve boş alanı denetle.",
    )
}

fn write_picked(
    app: &AppHandle,
    picked: tauri_plugin_fs::FilePath,
    ext: &str,
    bytes: Vec<u8>,
) -> AppResult<Option<String>> {
    use std::io::Write;
    use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
    match picked {
        FilePath::Url(url) => {
            let shown = url.to_string();
            let mut opts = OpenOptions::new();
            opts.write(true).truncate(true);
            let mut f = app.fs().open(FilePath::Url(url), opts).map_err(write_err)?;
            f.write_all(&bytes).map_err(write_err)?;
            f.flush().map_err(write_err)?;
            Ok(Some(shown))
        }
        FilePath::Path(path) => {
            let path = if ext.is_empty() {
                path
            } else {
                mh_app::files::with_extension(path, ext)
            };
            write_private(&path, &bytes).map_err(write_err)?;
            Ok(Some(path.to_string_lossy().into_owned()))
        }
    }
}

/// Postanın `index` numaralı eki (ad, tür, içerik); gövde yerelde yoksa
/// ortak katman indirir.
async fn attachment_data(
    state: &AppState,
    id: MessageId,
    index: u32,
) -> AppResult<mh_mime::AttachmentData> {
    ensure_body(state, id).await?;
    let raw = state
        .store
        .raw(id)?
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "Postanın içeriği yerelde yok."))?;
    mh_mime::attachment(&raw, index).ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "Ek bulunamadı; posta değişmiş olabilir.",
        )
    })
}

/// Eki önbelleğe (`ekler/<posta>/<ad>`) yazar ve paylaşım sayfasına verir.
#[tauri::command]
pub async fn attachment_share(
    app: AppHandle,
    state: State<'_, AppState>,
    id: MessageId,
    index: u32,
) -> Res<()> {
    let att = attachment_data(&state, id, index).await?;
    let dir = state.cache_dir.join("ekler").join(id.to_string());
    private_dir(&dir).map_err(internal)?;
    let path = dir.join(sanitize_filename(&att.name));
    write_private(&path, &att.bytes).map_err(|e| {
        tracing::warn!(error = %e, "ek yazılamadı");
        AppError::new(
            ErrorCode::Storage,
            "Ek diske yazılamadı. Boş alanı denetle.",
        )
    })?;
    share(&app, &path, &att.mime)
}

/// Eki kullanıcının seçtiği yere kaydeder (sistemin belge oluşturucusu);
/// seçilen adres, vazgeçilirse `None`.
#[tauri::command]
pub async fn attachment_save(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: MessageId,
    index: u32,
) -> Res<Option<String>> {
    let att = attachment_data(&state, id, index).await?;
    let name = sanitize_filename(&att.name);
    state.platform.save_file(&window, &name, att.bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suzgec_adi() {
        assert_eq!(filter_label("eml"), "E-posta (.eml)");
        assert_eq!(filter_label("ics"), ".ics");
    }

    #[test]
    fn yuzde_cozumu() {
        assert_eq!(percent_decode("rapor%20son.pdf"), "rapor son.pdf");
        assert_eq!(percent_decode("d%C3%BCz.txt"), "düz.txt");
        assert_eq!(percent_decode("bozuk%2"), "bozuk%2");
        assert_eq!(percent_decode("yok"), "yok");
    }
}
