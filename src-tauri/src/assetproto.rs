//! kamucl-asset 协议：受管图片只读服务（对应原版 protocol.handle('kamucl-asset')）。
//!
//! 原版授权口径（core/appearanceAssets.ts authorizeManagedImage）：只允许
//! userData/launcher-managed 下的受管文件。这里同样只服务受管根，拒绝一切穿越。

use std::path::PathBuf;
use tauri::UriSchemeContext;

const MANAGED_PREFIX: &str = "launcher-managed";

fn managed_root<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> PathBuf {
    crate::settings::user_data_dir(app).join(MANAGED_PREFIX)
}

/// GET kamucl-asset://…/image?path=<绝对路径或受管相对路径> → 文件字节
pub fn handle<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    if request.method() != "GET" {
        return tauri::http::Response::builder()
            .status(405)
            .body(b"Method Not Allowed".to_vec())
            .unwrap();
    }
    let app = ctx.app_handle();
    let url = request.uri().to_string();
    // Windows 下 scheme 呈现为 http://kamucl-asset.localhost/...，取 query 里的 path
    let Some(query) = request.uri().query() else {
        return not_found();
    };
    let path_param = form_urlencoded_get(query, "path");
    let Some(path_param) = path_param else {
        return not_found();
    };
    let _ = url;

    let candidate = PathBuf::from(&path_param);
    let root = managed_root(app);
    let authorized = normalize(&candidate).ok().and_then(|p| {
        if p.starts_with(&root) {
            Some(p)
        } else {
            None
        }
    });
    let Some(authorized) = authorized else {
        return not_found();
    };
    match std::fs::read(&authorized) {
        Ok(bytes) => {
            let mime = mime_of(&authorized);
            tauri::http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(bytes)
                .unwrap()
        }
        Err(_) => not_found(),
    }
}

fn not_found() -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(404)
        .body(b"Not Found".to_vec())
        .unwrap()
}

fn normalize(p: &std::path::Path) -> Result<PathBuf, ()> {
    // 先按字面规整（不解软链），保证不越出受管根即可
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    Ok(out)
}

fn form_urlencoded_get(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(urldecode(v));
        }
    }
    None
}

fn urldecode(s: &str) -> String {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push(((h << 4) | l) as u8);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn mime_of(p: &std::path::Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        _ => "application/octet-stream",
    }
}
