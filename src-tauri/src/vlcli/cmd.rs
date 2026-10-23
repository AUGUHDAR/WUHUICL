//! 一次性命令执行：拼参数、跑进程、解信封、区分退出码。

use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Failure {
    pub code: String,
    pub message: String,
    /// true 表示命令本身不对（退出码 2 / USAGE），升级 CLI 时会静默失效，要单独报
    pub usage: bool,
}

impl Failure {
    pub fn of(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into(), usage: false }
    }
}

/// 把渲染层传来的字段整理成 CLI 的 --json 负载。
pub fn json_payload(fields: &[(&str, Option<&Value>)]) -> String {
    let mut map = serde_json::Map::new();
    for (key, value) in fields {
        if let Some(v) = value {
            if !v.is_null() {
                map.insert((*key).to_string(), (*v).clone());
            }
        }
    }
    serde_json::Value::Object(map).to_string()
}

/// 纯函数：由退出码与 stdout 得出结果，便于单测。
pub fn interpret(code: Option<i32>, stdout: &str) -> Result<Value, Failure> {
    let line = stdout
        .lines()
        .rev()
        .map(|l| l.trim())
        .find(|l| l.starts_with('{'));
    if code == Some(2) {
        if let Ok(parsed) = serde_json::from_str::<Value>(line.unwrap_or("")) {
            let text = parsed["message"]
                .as_str()
                .or(parsed["error"].as_str())
                .unwrap_or("命令不被支持")
                .to_string();
            return Err(Failure { code: "USAGE".into(), message: text, usage: true });
        }
        let text = line.unwrap_or(stdout.trim()).chars().take(200).collect();
        return Err(Failure { code: "USAGE".into(), message: text, usage: true });
    }
    let Some(line) = line else {
        return Err(Failure::of("CLI_OUTPUT", "联机组件没有返回内容"));
    };
    let Ok(parsed) = serde_json::from_str::<Value>(line) else {
        return Err(Failure::of("CLI_OUTPUT", "联机组件返回了无法解读的内容"));
    };
    if parsed["ok"].as_bool() == Some(true) {
        return Ok(parsed.get("data").cloned().unwrap_or(Value::Null));
    }
    let api_code = parsed["code"].as_str().or(parsed["error"].as_str()).unwrap_or("CLI_ERROR");
    let message = parsed["message"]
        .as_str()
        .or(parsed["error"].as_str())
        .unwrap_or("联机操作未完成")
        .to_string();
    if api_code == "USAGE" {
        return Err(Failure { code: api_code.into(), message, usage: true });
    }
    Err(Failure { code: api_code.into(), message, usage: false })
}

pub fn run(bin: &Path, args: &[String], timeout: Duration) -> Result<Value, Failure> {
    if !bin.is_file() {
        return Err(Failure::of("CLI_MISSING", "联机组件尚未就绪"));
    }
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| Failure::of("CLI_SPAWN", format!("联机组件启动失败：{}", e)))?;
    let mut pipe = child
        .stdout
        .take()
        .ok_or_else(|| Failure::of("CLI_SPAWN", "联机组件输出不可读"))?;
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = pipe.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let stdout = match rx.recv_timeout(timeout) {
        Ok(s) => s,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Failure::of("CLI_TIMEOUT", "联机组件响应超时"));
        }
    };
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return Err(Failure::of("CLI_SPAWN", format!("联机组件退出异常：{}", e))),
    };
    interpret(status.code(), &stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_success_envelope() {
        let out = r#"{"ok":true,"data":{"rooms":[]}}"#;
        let v = interpret(Some(0), out).unwrap();
        assert_eq!(v["rooms"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn keeps_business_code() {
        let out = r#"{"ok":false,"error":"ROOM_NOT_FOUND","code":"ROOM_NOT_FOUND","message":"房号不存在或密码不对"}"#;
        let e = interpret(Some(1), out).unwrap_err();
        assert_eq!(e.code, "ROOM_NOT_FOUND");
        assert_eq!(e.message, "房号不存在或密码不对");
        assert!(!e.usage);
    }

    #[test]
    fn separates_usage_from_business() {
        let e = interpret(Some(2), "no such command \"room nope\"").unwrap_err();
        assert!(e.usage);
        assert_eq!(e.code, "USAGE");
    }

    #[test]
    fn flags_trailing_output_line() {
        let out = "noise before\n{\"ok\":true,\"data\":{\"state\":\"idle\"}}\n";
        assert_eq!(interpret(Some(0), out).unwrap()["state"], "idle");
    }

    #[test]
    fn usage_envelope_keeps_readable_message() {
        let out = r#"{"ok":false,"code":"USAGE","message":"invalid value \"2\" for flag -timeout"}"#;
        let e = interpret(Some(2), out).unwrap_err();
        assert!(e.usage);
        assert_eq!(e.code, "USAGE");
        assert!(e.message.contains("-timeout"), "实际 {}", e.message);
    }

    #[test]
    fn empty_output_is_not_success() {
        assert_eq!(interpret(Some(0), "").unwrap_err().code, "CLI_OUTPUT");
    }

    #[test]
    fn payload_drops_nulls() {
        let v = Value::from(25565);
        let s = json_payload(&[("name", None), ("hostPort", Some(&v)), ("password", Some(&Value::Null))]);
        assert_eq!(s, r#"{"hostPort":25565}"#);
    }
}
