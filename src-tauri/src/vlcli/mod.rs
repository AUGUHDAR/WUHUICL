//! VoxLink CLI 桥：通道入口，把界面请求翻译成一次性命令或常驻会话。

pub mod asset;
pub mod cmd;
pub mod session;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

const ONE_SHOT: Duration = Duration::from_secs(25);
const LONG: Duration = Duration::from_secs(900);

pub fn invoke(
    app: &tauri::AppHandle,
    channel: &str,
    args: &[Value],
) -> Option<Result<Value, String>> {
    if !channel.starts_with("vlcli:") {
        return None;
    }
    let first = args.first().cloned().unwrap_or(Value::Null);
    let result = match channel {
        "vlcli:ensure" => wrap(asset::ensure(
            app,
            first["force"].as_bool().unwrap_or(false),
        )),
        "vlcli:status" => Ok(json!({ "ok": true, "data": {
            "alive": session::alive(), "snapshot": session::snapshot() } })),
        "vlcli:stop" => wrap(session::stop()),
        "vlcli:capabilities" => wrap(capabilities(app)),
        "vlcli:pickFiles" => Ok(json!({ "ok": true, "data": { "files": pick_files(app) } })),
        "vlcli:lobby" => one_shot(
            app,
            "lobby list",
            &pick(
                &first,
                &[
                    ("page", "page"),
                    ("size", "size"),
                    ("category", "category"),
                    ("loader", "loader"),
                    ("search", "search"),
                ],
            ),
            ONE_SHOT,
        ),
        "vlcli:lobbyCategories" => one_shot(app, "lobby categories", "{}", ONE_SHOT),
        "vlcli:roomInfo" => one_shot(
            app,
            "room info",
            &pick(&first, &[("code", "code")]),
            ONE_SHOT,
        ),
        "vlcli:roomMods" => one_shot(
            app,
            "room mods",
            &pick(&first, &[("code", "code")]),
            ONE_SHOT,
        ),
        "vlcli:relayStatus" => one_shot(app, "relay status", "{}", ONE_SHOT),
        "vlcli:relayList" => one_shot(app, "relay list", "{}", ONE_SHOT),
        "vlcli:netPorts" => one_shot(app, "net ports", "{}", ONE_SHOT),
        "vlcli:settingsGet" => one_shot(app, "settings get", "{}", ONE_SHOT),
        "vlcli:settingsSave" => one_shot(
            app,
            "settings save",
            &pick(
                &first,
                &[
                    ("theme", "theme"),
                    ("clientTag", "clientTag"),
                    ("relayEnabled", "relayEnabled"),
                ],
            ),
            ONE_SHOT,
        ),
        "vlcli:host" => start(
            app,
            "room create",
            &pick(
                &first,
                &[
                    ("name", "name"),
                    ("hostPort", "hostPort"),
                    ("password", "password"),
                    ("category", "category"),
                    ("loader", "loader"),
                    ("gameVersion", "gameVersion"),
                    ("visible", "visible"),
                    ("clientTag", "clientTag"),
                ],
            ),
        ),
        "vlcli:join" => start(
            app,
            "room join",
            &pick(
                &first,
                &[
                    ("code", "code"),
                    ("password", "password"),
                    ("modsync", "modsync"),
                ],
            ),
        ),
        "vlcli:modsync" => modsync(app, &first),
        "vlcli:tickets" => tickets(app, &first),
        _ => Ok(json!({ "ok": false, "code": "UNKNOWN_CHANNEL", "message": "未知的联机通道" })),
    };
    Some(result)
}

/// 子系统自建的返回值统一套一层 data，跟组件返回的信封同构。
fn wrap(v: Value) -> Result<Value, String> {
    let ok = v["ok"].as_bool().unwrap_or(true);
    let mut data = v.clone();
    if let Some(obj) = data.as_object_mut() {
        obj.remove("ok");
    }
    Ok(json!({ "ok": ok, "data": data }))
}

fn binary(app: &tauri::AppHandle) -> PathBuf {
    asset::binary_path(app)
}

fn capabilities(app: &tauri::AppHandle) -> Value {
    let meta = asset::read_meta(app);
    let mut out = asset::probe(app);
    if let Some(obj) = out.as_object_mut() {
        obj.insert("cachedVersion".into(), meta["version"].clone());
        obj.insert("cachedBytes".into(), meta["bytes"].clone());
    }
    out
}

fn envelope(v: Result<Value, cmd::Failure>) -> Value {
    match v {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(e) => json!({ "ok": false, "code": e.code, "message": e.message, "usage": e.usage }),
    }
}

fn one_shot(
    app: &tauri::AppHandle,
    command: &str,
    payload: &str,
    timeout: Duration,
) -> Result<Value, String> {
    // 组件按 "域 动作" 两个参数解析，合成一个 token 会被判成未知命令
    let mut args: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    if payload != "{}" {
        args.push("--json".into());
        args.push(payload.into());
    }
    Ok(envelope(cmd::run(&binary(app), &args, timeout)))
}

fn start(app: &tauri::AppHandle, command: &str, payload: &str) -> Result<Value, String> {
    if command == "room join" {
        let parsed: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let code = parsed["code"].as_str().unwrap_or("");
        if sanitize_code(code).is_empty() {
            return Ok(
                json!({ "ok": false, "code": "INVALID_PARAMS", "message": "请输入有效的六位房间码" }),
            );
        }
    }
    session::start(&binary(app), command, payload)
        .map(|v| envelope(Ok(v)))
        .map_err(|e| e)
}

/// 房号校验：加入前只挡明显不合法的输入，其余交给组件判断。
fn sanitize_code(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.len() == 6 {
        cleaned.to_uppercase()
    } else {
        String::new()
    }
}

fn modsync(app: &tauri::AppHandle, arg: &Value) -> Result<Value, String> {
    let action = arg["action"].as_str().unwrap_or("state");
    let command = match action {
        "gate" => "modsync gate",
        "download" => "modsync download",
        "state" => "modsync state",
        "wait" => "modsync wait",
        "cancel" => "modsync cancel",
        "bypass" => "modsync bypass",
        "clear" => "modsync clear",
        "skipped" => "modsync skipped",
        "hostStatus" => "modsync host-status",
        "rebuild" => "modsync rebuild",
        "settings" => "modsync settings",
        other => {
            return Ok(
                json!({ "ok": false, "code": "USAGE", "message": format!("未知的模组同步动作 {}", other), "usage": true }),
            )
        }
    };
    let timeout = if action == "download" || action == "wait" {
        LONG
    } else {
        ONE_SHOT
    };
    one_shot(
        app,
        command,
        &pick(
            arg,
            &[
                ("code", "code"),
                ("scope", "scope"),
                ("timeoutSec", "timeoutSec"),
                ("downloadTimeoutSec", "downloadTimeoutSec"),
                ("keys", "keys"),
                ("jsonRaw", "jsonRaw"),
            ],
        ),
        timeout,
    )
}

fn tickets(app: &tauri::AppHandle, arg: &Value) -> Result<Value, String> {
    let action = arg["action"].as_str().unwrap_or("list");
    let command = match action {
        "list" => "ticket list",
        "poll" => "ticket poll",
        "detail" => "ticket detail",
        "submit" => "ticket submit",
        "reply" => "ticket reply",
        "retract" => "ticket retract",
        "viewed" => "ticket viewed",
        "delete" => "ticket delete",
        other => {
            return Ok(
                json!({ "ok": false, "code": "USAGE", "message": format!("未知的工单动作 {}", other), "usage": true }),
            )
        }
    };
    one_shot(
        app,
        command,
        &pick(
            arg,
            &[
                ("id", "id"),
                ("description", "description"),
                ("text", "text"),
                ("msg", "msg"),
                ("files", "files"),
            ],
        ),
        ONE_SHOT,
    )
}

/// 工单附件用系统文件框多选，路径交给组件的 --files。
fn pick_files(app: &tauri::AppHandle) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_files()
        .map(|list| list.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default()
}

/// 从界面参数里挑出组件认识的字段，改名后拼成 --json 负载。
fn pick(arg: &Value, map: &[(&str, &str)]) -> String {
    let mut out = serde_json::Map::new();
    for (from, to) in map {
        if let Some(v) = arg.get(*from) {
            if !v.is_null() {
                out.insert((*to).to_string(), v.clone());
            }
        }
    }
    Value::Object(out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_renames_and_drops_missing() {
        let arg = json!({ "code": "AB12CD", "password": null, "extra": 1 });
        assert_eq!(
            pick(&arg, &[("code", "code"), ("password", "password")]),
            r#"{"code":"AB12CD"}"#
        );
    }

    #[test]
    fn pick_keeps_zero_and_false() {
        let arg = json!({ "visible": false, "page": 0 });
        let s = pick(&arg, &[("visible", "visible"), ("page", "page")]);
        assert!(s.contains("false"));
        assert!(s.contains("0"));
    }

    #[test]
    fn code_sanitizer_accepts_only_six_alnum() {
        assert_eq!(sanitize_code("ab12cd"), "AB12CD");
        assert_eq!(sanitize_code("AB-12C D"), "AB12CD");
        assert_eq!(sanitize_code("12345"), "");
        assert_eq!(sanitize_code("1234567"), "");
    }

    #[test]
    fn failure_keeps_usage_flag() {
        let e = envelope(Err(cmd::Failure {
            code: "USAGE".into(),
            message: "没这条命令".into(),
            usage: true,
        }));
        assert_eq!(e["ok"], false);
        assert_eq!(e["usage"], true);
        assert_eq!(e["code"], "USAGE");
    }

    #[test]
    fn success_envelope_carries_data() {
        let e = envelope(Ok(json!({ "rooms": [] })));
        assert_eq!(e["ok"], true);
        assert!(e["data"]["rooms"].is_array());
    }
}
