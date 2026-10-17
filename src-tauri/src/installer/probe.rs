//! Installer path normalization and version location probing.

use std::path::{Path, PathBuf};

// ---------------- 通用工具 ----------------

pub fn basename_of(p: &str) -> String {
    Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// 对应原版 core/folderPaths.ts canonicalPath：存在时解析 junction/symlink，否则绝对规范化
pub fn canonical_path(input: &str) -> PathBuf {
    let trimmed = input.trim();
    let resolved = if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        std::env::current_dir().unwrap_or_default().join(trimmed)
    };
    strip_device_prefix(&std::fs::canonicalize(&resolved).unwrap_or(resolved))
}

/// 原版 path.resolve 永不产生 \\?\ 设备前缀，Windows canonicalize 会加，统一剥掉
pub fn strip_device_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!("\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p.to_path_buf()
}

/// 对应原版 pathIdentity：Windows 大小写不敏感 + 去尾部多余分隔符
pub fn path_identity(input: &str) -> String {
    let mut value = canonical_path(input).to_string_lossy().to_string();
    let root = Path::new(&value)
        .ancestors()
        .last()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    while value.len() > root.len() && (value.ends_with('\\') || value.ends_with('/')) {
        value.pop();
    }
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

pub fn same_path(a: &str, b: &str) -> bool {
    path_identity(a) == path_identity(b)
}

/// ipc.ts scopedVersion 用的 norm（path.resolve + 小写，不做 realpath）
pub(crate) fn resolve_norm(p: &str) -> String {
    let resolved = if Path::new(p).is_absolute() {
        PathBuf::from(p)
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    resolved.to_string_lossy().to_lowercase()
}

/// 对应原版 resolveMinecraftRoot：versions 目录 / 内嵌 .minecraft / 直接根目录
/// 返回 (路径, structure: minecraft|kamucl|empty)
pub fn resolve_minecraft_root(input: &str) -> Result<(String, &'static str), String> {
    if input.trim().is_empty() {
        return Err("文件夹路径不能为空".into());
    }
    let mut selected = canonical_path(input);
    if !selected.exists() {
        return Err("文件夹不存在，请检查路径是否正确".into());
    }
    if !selected.is_dir() {
        return Err("选择的路径不是文件夹".into());
    }
    if selected
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase() == "versions")
        .unwrap_or(false)
    {
        if let Some(parent) = selected.parent() {
            selected = parent.to_path_buf();
        }
    } else if !selected.join("versions").exists() {
        let nested = selected.join(".minecraft");
        if nested.join("versions").exists() {
            selected = nested;
        }
    }
    let structure = if selected.join(crate::installer::DATA_DIR_NAME).exists() {
        "kamucl"
    } else if selected.join("versions").exists() {
        "minecraft"
    } else {
        "empty"
    };
    Ok((selected.to_string_lossy().to_string(), structure))
}
