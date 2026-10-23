//! 对应原版 core/instanceMetadata.ts 的 resolveInstanceMetadata()；只读版本 json 的真实启动元数据。

use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::community::{normalize_loader, normalize_loader_version};

pub struct Resolved {
    pub mc_version: String,
    pub loader: Option<&'static str>,
    pub loader_version: Option<String>,
    pub broken: bool,
}

fn mc_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:[1-9]\d*\.\d+(?:\.\d+)?(?:-(?:pre|rc)-?\d+|-snapshot-\d+| Pre-Release \d+)?|\d{2}w\d{2}[a-z](?:[\w.-]*)|[ab]\d+(?:\.\d+)+[a-z]?(?:_\d+|-pre\d+)?|c\d+(?:\.\d+)+[a-z]?(?:_[\w-]+)?|rd-\d+|inf-\d+|1\.RV-Pre1|3D Shareware v1\.34)$",
        )
        .unwrap()
    })
}

pub fn is_minecraft_version_id(value: Option<&str>) -> bool {
    let Some(v) = value else { return false };
    if v.is_empty() || v.trim() != v || v == "0.0.0" {
        return false;
    }
    mc_id_re().is_match(v)
}

fn truthy<'a>(j: &'a Value, key: &str) -> Option<&'a str> {
    j.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn filled(value: &Option<String>) -> bool {
    value.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
}

fn game_args(chain: &[Value]) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#""[^"]*"|\S+"#).unwrap());
    let mut out = Vec::new();
    for j in chain {
        if let Some(game) = j.get("arguments").and_then(|a| a.get("game")).and_then(Value::as_array) {
            for a in game {
                match a {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        if let Some(value) = other.get("value") {
                            match value {
                                Value::String(s) => out.push(s.clone()),
                                Value::Array(arr) => {
                                    for x in arr.iter().filter_map(Value::as_str) {
                                        out.push(x.to_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        if let Some(text) = j.get("minecraftArguments").and_then(Value::as_str) {
            for m in re.find_iter(text) {
                let mut token = m.as_str().to_string();
                if token.starts_with('"') {
                    token.remove(0);
                }
                if token.ends_with('"') {
                    token.pop();
                }
                out.push(token);
            }
        }
    }
    out
}

pub fn resolve_instance_metadata(
    json: &Value,
    read_parent: &dyn Fn(&str) -> Option<Value>,
    read_client_version: Option<&dyn Fn(&[Value]) -> Option<String>>,
) -> Resolved {
    let mut chain: Vec<Value> = vec![json.clone()];
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(id) = json.get("id").and_then(Value::as_str) {
        seen.insert(id.to_string());
    }
    let mut broken = false;
    loop {
        let last = chain.last().cloned().unwrap_or(Value::Null);
        let Some(parent_id) = truthy(&last, "inheritsFrom").map(str::to_string) else {
            break;
        };
        if seen.contains(&parent_id)
            || chain.len() >= 16
            || parent_id.contains(['\\', '/', ':', '\0'])
            || parent_id == "."
            || parent_id == ".."
        {
            broken = true;
            break;
        }
        seen.insert(parent_id.clone());
        match read_parent(parent_id.as_str()) {
            Some(parent) => chain.push(parent),
            None => {
                broken = true;
                break;
            }
        }
    }

    let args = game_args(&chain);
    let arg = |name: &str| -> Option<String> {
        if let Some(index) = args.iter().position(|a| a == name) {
            return args.get(index + 1).cloned();
        }
        let prefix = format!("{name}=");
        args.iter().find(|a| a.starts_with(prefix.as_str())).map(|a| a[prefix.len()..].to_string())
    };
    let libraries: Vec<Vec<String>> = chain
        .iter()
        .flat_map(|j| {
            j.get("libraries")
                .and_then(Value::as_array)
                .map(|list| list.to_vec())
                .unwrap_or_default()
        })
        .map(|l| {
            l.get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split(':')
                .map(str::to_string)
                .collect()
        })
        .collect();
    let lib = |group: &str, artifact: &str| -> Option<String> {
        libraries
            .iter()
            .find(|parts| parts.first().map(|g| g == group).unwrap_or(false) && parts.get(1).map(|a| a == artifact).unwrap_or(false))
            .and_then(|parts| parts.get(2).cloned())
    };

    let detected = if filled(&arg("--fml.neoForgeVersion"))
        || filled(&lib("net.neoforged", "neoforge"))
        || filled(&lib("net.neoforged.fancymodloader", "loader"))
    {
        Some("neoforge")
    } else if filled(&arg("--fml.forgeVersion"))
        || filled(&lib("net.minecraftforge", "forge"))
        || filled(&lib("net.minecraftforge", "fmlloader"))
    {
        Some("forge")
    } else if filled(&lib("net.fabricmc", "fabric-loader")) {
        Some("fabric")
    } else if filled(&lib("org.quiltmc", "quilt-loader")) {
        Some("quilt")
    } else {
        None
    };
    let main = chain
        .iter()
        .map(|j| j.get("mainClass").and_then(Value::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let loader = detected
        .or_else(|| normalize_loader(chain.iter().find_map(|j| truthy(j, "_loader"))))
        .or_else(|| {
            if main.contains("neoforged") {
                Some("neoforge")
            } else if main.contains("minecraftforge") {
                Some("forge")
            } else if main.contains("fabricmc") {
                Some("fabric")
            } else if main.contains("quiltmc") {
                Some("quilt")
            } else {
                None
            }
        });
    let forge = lib("net.minecraftforge", "forge").or_else(|| lib("net.minecraftforge", "fmlloader"));
    let base = chain.last().cloned().unwrap_or(Value::Null);
    let vanilla_id = if truthy(&base, "inheritsFrom").is_none() && (loader.is_none() || chain.len() > 1) {
        base.get("id").and_then(Value::as_str).map(str::to_string)
    } else {
        None
    };
    let mut candidates: Vec<Option<String>> = vec![arg("--fml.mcVersion")];
    candidates.extend(chain.iter().map(|j| j.get("clientVersion").and_then(Value::as_str).map(str::to_string)));
    candidates.extend(chain.iter().map(|j| j.get("_mcVersion").and_then(Value::as_str).map(str::to_string)));
    candidates.push(vanilla_id);
    candidates.extend(chain.iter().map(|j| j.get("inheritsFrom").and_then(Value::as_str).map(str::to_string)));
    candidates.push(lib("net.fabricmc", "intermediary"));
    candidates.push(lib("org.quiltmc", "hashed"));
    candidates.push(
        forge
            .as_deref()
            .filter(|f| f.contains('-'))
            .map(|f| f.split('-').next().unwrap_or("").to_string()),
    );
    let mut mc_version = candidates
        .into_iter()
        .flatten()
        .find(|c| is_minecraft_version_id(Some(c.as_str())))
        .unwrap_or_default();
    if mc_version.is_empty() {
        if let Some(read) = read_client_version {
            if let Some(evidence) = read(chain.as_slice()) {
                if is_minecraft_version_id(Some(evidence.as_str())) {
                    mc_version = evidence;
                }
            }
        }
    }
    if mc_version.is_empty() {
        mc_version = "未知".to_string();
    }
    let raw_loader = match loader {
        Some("neoforge") => arg("--fml.neoForgeVersion").or_else(|| lib("net.neoforged", "neoforge")),
        Some("forge") => arg("--fml.forgeVersion").or_else(|| forge.clone()),
        Some("fabric") => lib("net.fabricmc", "fabric-loader"),
        Some("quilt") => lib("org.quiltmc", "quilt-loader"),
        _ => None,
    }
    .or_else(|| chain.iter().find_map(|j| truthy(j, "_loaderVersion")).map(str::to_string));
    let loader_version = raw_loader
        .filter(|raw| !raw.is_empty())
        .map(|raw| normalize_loader_version(&raw, loader, Some(mc_version.as_str())));
    Resolved {
        mc_version,
        loader,
        loader_version,
        broken,
    }
}
