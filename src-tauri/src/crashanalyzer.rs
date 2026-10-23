//! 崩溃分析器（PCL `Modules/Minecraft/ModCrash.vb` 的移植）。
//!
//! 三级流水线，与 PCL 一致：
//! 1. Crit1：精准日志匹配（中/高优先级，Contains 规则 + 少量正则提取细节）
//! 2. Crit2：Mixin / Suspected Mods / Fabric 解决方案（ crit1 无结果时）
//! 3. 堆栈关键字分析（仅疑似装了 Mod 时）：从堆栈提取包名候选 → 反查 Mod 文件名
//! 4. Crit3：精准日志匹配（低优先级）
//!
//! 移植说明：.NET 的后行环视 `(?<=…)` 在 Rust regex 里不可用，全部改写为
//! 捕获组取 group(1)；规则命中顺序与 PCL 相同，原因名保持 PCL 原文。

use serde_json::json;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 一条可能的崩溃原因（title 为 PCL 原文，details 为附加信息如 Mod 文件名）
#[derive(Debug, Clone)]
pub struct CrashReason {
    pub title: String,
    pub details: Vec<String>,
}

/// 收集到的四类日志（None = 没有）
pub struct Logs {
    /// Minecraft 日志（logs/latest.log 或启动器捕获的游戏输出）
    pub mc: Option<String>,
    /// JVM 致命错误日志（hs_err_pid*.log）
    pub hs: Option<String>,
    /// 本次崩溃报告（crash-reports/crash-*.txt）
    pub crash: Option<String>,
    /// Minecraft 调试日志（logs/debug.log，用于 Mod 名反查）
    pub debug: Option<String>,
}

pub const REASON_NONE: &str = "没有可用的分析文件";

/// 从游戏目录收集日志并分析。`crash_path` 是启动器 diff 出的本次新崩溃报告（可空）。
pub fn analyze(game_dir: &Path, crash_path: Option<&Path>, captured_tail: &[String]) -> Vec<CrashReason> {
    let mc = read_recent(&game_dir.join("logs").join("latest.log")).or_else(|| {
        // 没有落盘日志时用启动器捕获的输出兜底（PCL 的 RawOutput 语义）
        if captured_tail.is_empty() {
            None
        } else {
            Some(captured_tail.join("\n"))
        }
    });
    let mc = match (&mc, captured_tail.is_empty()) {
        (Some(_), false) => {
            // 两者都有：把捕获输出拼在末尾（PCL 把 RawOutput 也纳入分析文件）
            let mut all = mc.unwrap_or_default();
            all.push('\n');
            all.push_str(&captured_tail.join("\n"));
            Some(all)
        }
        _ => mc,
    };
    let hs = newest_file(game_dir, "hs_err_pid*.log").and_then(|p| read_recent(&p));
    let crash = crash_path.and_then(read_recent);
    let debug = read_recent(&game_dir.join("logs").join("debug.log"));

    let logs = Logs { mc, hs, crash, debug };
    if logs.mc.is_none() && logs.hs.is_none() && logs.crash.is_none() {
        return vec![CrashReason { title: REASON_NONE.into(), details: vec![] }];
    }
    analyze_logs(&logs)
}

// ---------------- 文件收集 ----------------

fn read_recent(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    // 只在 3 分钟内修改过才算本次崩溃的日志（PCL Collect 的 3 分钟口径放宽到 10 分钟：
    // 启动器在游戏退出瞬间分析，正常都会命中；手动分析场景留裕量）
    let fresh = meta
        .modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < 600)
        .unwrap_or(false);
    if !fresh {
        return None;
    }
    let mut f = std::fs::File::open(path).ok()?;
    // 上限 4MB：崩溃报告/日志超过部分对规则匹配意义不大
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut text = Vec::new();
    let _ = f.read_to_end(&mut text);
    text.truncate(buf.len());
    drop(buf);
    Some(String::from_utf8_lossy(&text).to_string())
}

fn newest_file(dir: &Path, pattern: &str) -> Option<PathBuf> {
    let prefix = pattern.split('*').next()?.to_string();
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !p.is_file() {
            continue;
        }
        let mtime = e.metadata().ok()?.modified().ok()?;
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, p));
        }
    }
    best.map(|(_, p)| p)
}

// ---------------- 主流水线 ----------------

pub fn analyze_logs(logs: &Logs) -> Vec<CrashReason> {
    let mut reasons: Vec<CrashReason> = Vec::new();

    crit1(logs, &mut reasons);
    if reasons.is_empty() {
        crit2(logs, &mut reasons);
    }
    if reasons.is_empty() {
        stack_analysis(logs, &mut reasons);
    }
    if reasons.is_empty() {
        crit3(logs, &mut reasons);
    }
    reasons
}

fn push(reasons: &mut Vec<CrashReason>, title: &str, details: Vec<String>) {
    let details: Vec<String> = details.into_iter().filter(|d| !d.trim().is_empty()).collect();
    if let Some(existing) = reasons.iter_mut().find(|r| r.title == title) {
        for d in details {
            if !existing.details.contains(&d) {
                existing.details.push(d);
            }
        }
    } else {
        reasons.push(CrashReason { title: title.to_string(), details });
    }
}

fn has(text: &Option<String>, needle: &str) -> bool {
    text.as_deref().map(|t| t.contains(needle)).unwrap_or(false)
}

fn seek(text: &Option<String>, re: &regex::Regex) -> Option<String> {
    text.as_deref()
        .and_then(|t| re.captures(t))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

fn seek_all(text: &Option<String>, re: &regex::Regex) -> Vec<String> {
    text.as_deref()
        .map(|t| {
            re.captures_iter(t)
                .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

// ---------------- Crit1：精准日志匹配（中/高优先级） ----------------

/// (命中关键字, 原因) —— 与 PCL AnalyzeCrit1 的 Contains 规则一一对应
const CRIT1_MC_CONTAINS: &[(&str, &str)] = &[
    ("Unrecognized option:", "Java虚拟机参数有误"),
    (
        "Found multiple arguments for option fml.forgeVersion, but you asked for only one",
        "版本Json中存在多个Forge",
    ),
    ("The driver does not appear to support OpenGL", "显卡不支持OpenGL"),
    ("java.lang.ClassCastException: java.base/jdk", "使用JDK"),
    ("java.lang.ClassCastException: class jdk.", "使用JDK"),
    (
        "java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.texture.SpriteContents.<init>",
        "OptiFine与Forge不兼容",
    ),
    (
        "java.lang.NoSuchMethodError: 'java.lang.String com.mojang.blaze3d.systems.RenderSystem.getBackendDescription",
        "OptiFine与Forge不兼容",
    ),
    (
        "java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.block.model.BakedQuad.<init>",
        "OptiFine与Forge不兼容",
    ),
    (
        "java.lang.NoSuchMethodError: 'void net.minecraftforge.client.gui.overlay.ForgeGui.renderSelectedItemName",
        "OptiFine与Forge不兼容",
    ),
    ("java.lang.NoSuchMethodError: 'void net.minecraft.server.level.DistanceManager", "OptiFine与Forge不兼容"),
    (
        "java.lang.NoSuchMethodError: 'net.minecraft.network.chat.FormattedText net.minecraft.client.gui.Font.ellipsize",
        "OptiFine与Forge不兼容",
    ),
    ("Open J9 is not supported", "使用OpenJ9"),
    ("OpenJ9 is incompatible", "使用OpenJ9"),
    (".J9VMInternals.", "使用OpenJ9"),
    ("java.lang.NoSuchFieldException: ucp", "Java版本过高"),
    ("because module java.base does not export", "Java版本过高"),
    (
        "java.lang.ClassNotFoundException: jdk.nashorn.api.scripting.NashornScriptEngineFactory",
        "Java版本过高",
    ),
    (
        "java.lang.ClassNotFoundException: java.lang.invoke.LambdaMetafactory",
        "Java版本过高",
    ),
    (
        "The directories below appear to be extracted jar files. Fix this before you continue.",
        "Mod文件被解压",
    ),
    ("Extracted mod jars found, loading will NOT continue", "Mod文件被解压"),
    (
        "java.lang.ClassNotFoundException: org.spongepowered.asm.launch.MixinTweaker",
        "MixinBootstrap缺失",
    ),
    ("Couldn't set pixel format", "显卡驱动不支持导致无法设置像素格式"),
    ("java.lang.OutOfMemoryError", "内存不足"),
    ("an out of memory error", "内存不足"),
    (
        "java.lang.RuntimeException: Shaders Mod detected. Please remove it, OptiFine has built-in support for shaders.",
        "ShadersMod与OptiFine同时安装",
    ),
    (
        "java.lang.NoSuchMethodError: sun.security.util.ManifestEntryVerifier",
        "低版本Forge与高版本Java不兼容",
    ),
    ("1282: Invalid operation", "光影或资源包导致OpenGL1282错误"),
    (
        "signer information does not match signer information of other classes in the same package",
        "文件或内容校验失败",
    ),
    ("Maybe try a lower resolution resourcepack?", "材质过大或显卡配置不足"),
    ("Invalid maximum heap size", "使用32位Java导致JVM无法分配足够多的内存"),
    (
        "has been compiled by a more recent version of the Java Runtime (class file version 55.0)",
        "Mod需要Java11",
    ),
    (
        "java.lang.RuntimeException: java.lang.NoSuchMethodException: no such method: sun.misc.Unsafe.defineAnonymousClass",
        "Mod需要Java11",
    ),
    (
        "java.lang.IllegalArgumentException: The requested compatibility level JAVA_11 could not be set",
        "Mod需要Java11",
    ),
    ("Unsupported class file major version", "Java版本不兼容"),
    ("Unsupported major.minor version", "Java版本不兼容"),
    ("Level is not supported by the active JRE or ASM version", "Java版本不兼容"),
    ("Invalid module name: '' is not a Java identifier", "Mod名称包含特殊字符"),
    ("Cannot find launch target fmlclient, unable to launch", "Forge安装不完整"),
    ("DuplicateModsFoundException", "Mod重复安装"),
    ("Found a duplicate mod", "Mod重复安装"),
    ("Found duplicate mods", "Mod重复安装"),
    ("ModResolutionException: Duplicate", "Mod重复安装"),
];

const CRIT1_CRASH_CONTAINS: &[(&str, &str)] = &[
    ("maximum id range exceeded", "Mod过多导致超出ID限制"),
    ("java.lang.OutOfMemoryError", "内存不足"),
    ("Pixel format not accelerated", "显卡驱动不支持导致无法设置像素格式"),
    ("Manually triggered debug crash", "玩家手动触发调试崩溃"),
    (
        "com.electronwill.nightconfig.core.io.ParsingException: Not enough data available",
        "NightConfig的Bug",
    ),
    ("Multiple entries with same key: ", "确定Mod导致游戏崩溃"),
    ("LoaderExceptionModCrash: Caught exception from ", "确定Mod导致游戏崩溃"),
    ("Failed loading config file ", "Mod配置文件导致游戏崩溃"),
];

const CRIT1_HS_CONTAINS: &[(&str, &str)] = &[
    ("The system is out of physical RAM or swap space", "内存不足"),
    ("Out of Memory Error", "内存不足"),
];

fn crit1(logs: &Logs, reasons: &mut Vec<CrashReason>) {
    // 空白分析
    if logs.mc.is_none() && logs.hs.is_none() && logs.crash.is_none() {
        push(reasons, REASON_NONE, vec![]);
        return;
    }

    // ---- 游戏日志 ----
    for (needle, reason) in CRIT1_MC_CONTAINS {
        if has(&logs.mc, needle) {
            push(reasons, reason, vec![]);
        }
    }
    if has(&logs.mc, "Could not reserve enough space") {
        if has(&logs.mc, "for 1048576KB object heap") {
            push(reasons, "使用32位Java导致JVM无法分配足够多的内存", vec![]);
        } else {
            push(reasons, "内存不足", vec![]);
        }
    }
    // OptiFine 导致无法加载世界（两条件同时成立）
    if has(&logs.mc, "java.lang.NoSuchMethodError: net.minecraft.world.server.ChunkManager$ProxyTicketManager.shouldForceTicks(J)Z")
        && has(&logs.mc, "OptiFine")
    {
        push(reasons, "OptiFine导致无法加载世界", vec![]);
    }
    // 文件或内容校验失败：提取冲突类名
    if has(&logs.mc, "signer information does not match") {
        let re = regex::Regex::new(r#"class "([^']+)"'s signer information"#).unwrap();
        if let Some(class) = seek(&logs.mc, &re) {
            push(reasons, "文件或内容校验失败", vec![class]);
        }
    }
    // 确定Mod导致游戏崩溃：Caught exception from <modid>
    if has(&logs.mc, "Caught exception from ") {
        let re = regex::Regex::new(r"Caught exception from ([^\r\n]+)").unwrap();
        if let Some(name) = seek(&logs.mc, &re) {
            let names = try_analyze_mod_name(logs, vec![name.trim().to_string()]);
            push(reasons, "确定Mod导致游戏崩溃", names);
        }
    }
    // Mod 重复安装细节
    if has(&logs.mc, "DuplicateModsFoundException") {
        let re = regex::Regex::new(r"\n\t\w+ : [A-Z]:[^\r\n]+[/\\]([^/\\\r\n]+?\.jar)").unwrap();
        push(reasons, "Mod重复安装", seek_all(&logs.mc, &re));
    }
    if has(&logs.mc, "Found a duplicate mod") {
        let re = regex::Regex::new(r"Found a duplicate mod[^\r\n]*").unwrap();
        if let Some(line) = seek(&logs.mc, &re) {
            let re_jar = regex::Regex::new(r"[^\\/:]+\.jar").unwrap();
            push(reasons, "Mod重复安装", vec![re_jar.find(&line).map(|m| m.as_str().to_string())].into_iter().flatten().collect());
        }
    }
    if has(&logs.mc, "Found duplicate mods") {
        let re = regex::Regex::new(r"Mod ID: '(\w+?)' from mod files:").unwrap();
        let ids: Vec<String> = dedup(seek_all(&logs.mc, &re));
        push(reasons, "Mod重复安装", ids);
    }
    if has(&logs.mc, "ModResolutionException: Duplicate") {
        let re = regex::Regex::new(r"ModResolutionException: Duplicate[^\r\n]*").unwrap();
        if let Some(line) = seek(&logs.mc, &re) {
            let re_jar = regex::Regex::new(r"[^\\/:;]+\.jar").unwrap();
            push(
                reasons,
                "Mod重复安装",
                re_jar.find_iter(&line).map(|m| m.as_str().to_string()).collect(),
            );
        }
    }
    // Mod 互不兼容（Fabric 报错正文）
    if has(&logs.mc, "Incompatible mods found!") {
        let detail = incompatible_mods_detail(logs.mc.as_deref().unwrap_or(""));
        push(reasons, "Mod互不兼容", vec![detail]);
    }
    // 新版 Fabric 的发现失败（PCL 只有老文案 "Mod resolution failed"；实测 0.20 用
    // "Mod discovery failed!"，且正文有 Error analyzing [坏jar路径]）
    if has(&logs.mc, "Mod discovery failed!") {
        let re = regex::Regex::new(r"Error analyzing \[([^\]]+)\]").unwrap();
        let jars: Vec<String> = seek_all(&logs.mc, &re);
        let names = try_analyze_mod_name(logs, jars.clone());
        push(reasons, "Mod加载器报错", names);
        push(reasons, "Mod加载器报错", jars);
    }
    // Mod 缺少前置
    if has(&logs.mc, "Missing or unsupported mandatory dependencies:") {
        let re = regex::Regex::new(r"Missing or unsupported mandatory dependencies:((?:\r?\n\t.*)+)").unwrap();
        if let Some(block) = seek(&logs.mc, &re) {
            let lines: Vec<String> = dedup(
                block
                    .lines()
                    .map(|l| l.trim_matches(|c: char| c == '\r' || c == '\n' || c == '\t' || c == ' ').to_string())
                    .collect(),
            );
            push(reasons, "Mod缺少前置或MC版本错误", lines);
        }
    }
    // Forge 安装不完整（fmlcore 缺失特征）
    if has(&logs.mc, "Invalid paths argument, contained no existing paths")
        && (has(&logs.mc, "libraries\\net\\minecraftforge\\fmlcore")
            || has(&logs.mc, "libraries/net/minecraftforge/fmlcore"))
    {
        push(reasons, "Forge安装不完整", vec![]);
    }

    // ---- 虚拟机日志 ----
    for (needle, reason) in CRIT1_HS_CONTAINS {
        if has(&logs.hs, needle) {
            push(reasons, reason, vec![]);
        }
    }
    if has(&logs.hs, "EXCEPTION_ACCESS_VIOLATION") {
        if has(&logs.hs, "# C  [ig") {
            push(reasons, "Intel驱动不兼容导致EXCEPTION_ACCESS_VIOLATION", vec![]);
        }
        if has(&logs.hs, "# C  [atio") {
            push(reasons, "AMD驱动不兼容导致EXCEPTION_ACCESS_VIOLATION", vec![]);
        }
        if has(&logs.hs, "# C  [nvoglv") {
            push(reasons, "Nvidia驱动不兼容导致EXCEPTION_ACCESS_VIOLATION", vec![]);
        }
    }

    // ---- 崩溃报告 ----
    for (needle, reason) in CRIT1_CRASH_CONTAINS {
        if has(&logs.crash, needle) {
            // NightConfig 的 Bug 与 Mod 配文件崩溃互斥（PCL: 若已判配置文件则不判 NightConfig）
            if *reason == "NightConfig的Bug" && reasons.iter().any(|r| r.title == "Mod配置文件导致游戏崩溃") {
                continue;
            }
            push(reasons, reason, vec![]);
        }
    }
    if has(&logs.crash, "has mods that were not found") {
        let re = regex::Regex::new(r"The Mod File [^\r\n]+optifine\\OptiFine[^\r\n]+ has mods that were not found").unwrap();
        if logs.crash.as_deref().map(|t| re.is_match(t)).unwrap_or(false) {
            push(reasons, "OptiFine与Forge不兼容", vec![]);
        }
    }
    if has(&logs.crash, "-- MOD ") {
        if let Some(seg) = between(logs.crash.as_deref(), "-- MOD ", "Failure message:") {
            if seg.to_lowercase().contains(".jar") {
                let re = regex::Regex::new(r"Mod File: (.+)").unwrap();
                let name = re
                    .captures(&seg)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().trim_matches(|c: char| c == '\r' || c == '\n' || c == ' ').to_string())
                    .unwrap_or_default();
                push(reasons, "确定Mod导致游戏崩溃", vec![name]);
            } else {
                let re = regex::Regex::new(r"Failure message: ([\s\S]*?)\n\tMod").unwrap();
                if let Some(msg) = seek(&logs.crash, &re) {
                    push(
                        reasons,
                        "Mod加载器报错",
                        vec![msg.replace('\t', " ").trim().to_string()],
                    );
                }
            }
        }
    }
    if has(&logs.crash, "Multiple entries with same key: ") {
        let re = regex::Regex::new(r"Multiple entries with same key: ([^=\r\n]+)").unwrap();
        if let Some(kw) = seek(&logs.crash, &re) {
            let names = try_analyze_mod_name(logs, vec![kw.trim().to_string()]);
            push(reasons, "确定Mod导致游戏崩溃", names);
        }
    }
    if has(&logs.crash, "LoaderExceptionModCrash: Caught exception from ") {
        let re = regex::Regex::new(r"LoaderExceptionModCrash: Caught exception from ([^\r\n]+)").unwrap();
        if let Some(name) = seek(&logs.crash, &re) {
            let names = try_analyze_mod_name(logs, vec![name.trim().to_string()]);
            push(reasons, "确定Mod导致游戏崩溃", names);
        }
    }
    if has(&logs.crash, "Failed loading config file ") {
        let re_id = regex::Regex::new(r"Failed loading config file .+ for modid ([^\r\n]+)").unwrap();
        let re_type = regex::Regex::new(r"Failed loading config file (.+?) of type").unwrap();
        let mut details = vec![seek(&logs.crash, &re_id).map(|s| s.trim().to_string()).unwrap_or_default()];
        if let Some(t) = seek(&logs.crash, &re_type) {
            details.push(t.trim().to_string());
        }
        let names = try_analyze_mod_name(logs, details);
        push(reasons, "Mod配置文件导致游戏崩溃", names);
    }
}

/// Incompatible mods found! 的正文提取（PCL 用后行环视，这里手工切片）
fn incompatible_mods_detail(mc: &str) -> String {
    let Some(start) = mc.find("Incompatible mods found!") else {
        return String::new();
    };
    let rest = &mc[start..];
    let end = rest.find("\n\tat ").unwrap_or(rest.len());
    let mut seg = rest[..end].to_string();
    if let Some(pos) = seg.find(": ") {
        seg = seg[pos + 2..].to_string();
    }
    if let Some(pos) = seg.find("更多信息：") {
        seg.truncate(pos);
    }
    seg.replace("Some of your mods are incompatible with the game or each other!", "")
        .trim()
        .to_string()
}

// ---------------- Crit2：Mixin / Suspected / Fabric 解决方案 ----------------

fn mixin_analyze(text: &Option<String>, logs: &Logs, reasons: &mut Vec<CrashReason>) -> bool {
    let is_mixin = has(text, "Mixin prepare failed ")
        || has(text, "Mixin apply failed ")
        || has(text, "MixinApplyError")
        || has(text, "MixinTransformerError")
        || has(text, "mixin.injection.throwables.")
        || has(text, ".json] FAILED during )");
    if !is_mixin {
        return false;
    }
    // Mod 名称匹配
    let re_mod = regex::Regex::new(r"from mod ([^./\s ]+)\] from").unwrap();
    let re_mod2 = regex::Regex::new(r"for mod ([^./\s ]+) failed").unwrap();
    let mod_name = seek(text, &re_mod).or_else(|| seek(text, &re_mod2));
    if let Some(name) = mod_name {
        let names = try_analyze_mod_name(logs, vec![name.trim().to_string()]);
        push(reasons, "ModMixin失败", names);
        return true;
    }
    // JSON 名称匹配
    let re_json = regex::Regex::new(r"[ \[{(]([^ \[{(]+\.[^ ]+?)\.json").unwrap();
    let jsons = seek_all(text, &re_json);
    if let Some(json_name) = jsons.first() {
        let kw = json_name.replace("mixins", "mixin").replace(".mixin", "").replace("mixin.", "");
        let names = try_analyze_mod_name(logs, vec![kw]);
        push(reasons, "ModMixin失败", names);
        return true;
    }
    push(reasons, "ModMixin失败", vec![]);
    true
}

fn crit2(logs: &Logs, reasons: &mut Vec<CrashReason>) {
    // 游戏日志
    let is_mixin = mixin_analyze(&logs.mc, logs, reasons);
    if has(&logs.mc, "An exception was thrown, the game will display an error screen and halt.") {
        let re = regex::Regex::new(
            r"the game will display an error screen and halt\.\r?\n[^\r\n]+?Exception: ([\s\S]*?)\r?\n\tat",
        )
        .unwrap();
        if let Some(detail) = seek(&logs.mc, &re) {
            push(reasons, "Forge报错", vec![detail.trim().to_string()]);
        }
    }
    for marker in [
        "A potential solution has been determined:\n",
        "A potential solution has been determined, this may resolve your problem:\n",
        "确定了一种可能的解决方法，这样做可能会解决你的问题：\n",
    ] {
        if has(&logs.mc, marker) {
            let text = logs.mc.as_deref().unwrap_or("");
            let Some(pos) = text.find(marker) else { continue };
            let rest = &text[pos + marker.len()..];
            let solutions: Vec<String> = rest
                .lines()
                .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
                .filter_map(|l| {
                    let t = l.trim();
                    (!t.is_empty()).then(|| t.to_string())
                })
                .collect();
            push(reasons, "Fabric报错并给出解决方案", solutions);
        }
    }
    if !is_mixin && has(&logs.mc, "due to errors, provided by '") {
        let re = regex::Regex::new(r"due to errors, provided by '([^']+)'").unwrap();
        if let Some(name) = seek(&logs.mc, &re) {
            let names = try_analyze_mod_name(logs, vec![name.trim().to_string()]);
            push(reasons, "确定Mod导致游戏崩溃", names);
        }
    }

    // 崩溃报告
    mixin_analyze(&logs.crash, logs, reasons);
    if has(&logs.crash, "Suspected Mod") {
        if let Some(seg) = between(logs.crash.as_deref(), "Suspected Mod", "Stacktrace") {
            if !seg.starts_with("s: None") {
                let re = regex::Regex::new(r"\n\t[^\(\t]+\(([^)\n]+)").unwrap();
                let suspects: Vec<String> = re
                    .captures_iter(&seg)
                    .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
                    .collect();
                if !suspects.is_empty() {
                    let names = try_analyze_mod_name(logs, suspects);
                    push(reasons, "怀疑Mod导致游戏崩溃", names);
                }
            }
        }
    }
}

// ---------------- 堆栈关键字分析 ----------------

const IGNORE_STACK_PREFIXES: &[&str] = &[
    "java", "sun", "javax", "jdk", "oolloo", "org.lwjgl", "com.sun", "net.minecraftforge",
    "paulscode.sound", "com.mojang", "net.minecraft", "cpw.mods", "com.google", "org.apache",
    "org.spongepowered", "net.fabricmc", "com.mumfrey", "com.electronwill.nightconfig",
    "it.unimi.dsi", "MojangTricksIntelDriversForPerformance_javaw",
];

/// 从堆栈提取 Mod 包名候选（PCL AnalyzeStackKeyword 的等价实现：堆栈行 → 去掉类名的包路径）
fn analyze_stack_keyword(error_stack: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in error_stack.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("at ") else { continue };
        let Some(open) = rest.find('(') else { continue };
        let fqcn = &rest[..open];
        let mut segments: Vec<&str> = fqcn.split('.').collect();
        if segments.len() < 3 {
            continue;
        }
        segments.pop(); // 方法名
        let class = segments.pop().unwrap_or("");
        // Mixin 堆栈：xxx$Bar$Baz 形式把内部类名也作为候选（$ → .）
        if class.contains('$') {
            out.push(class.replace('$', "."));
        }
        let package = segments.join(".");
        if !package.is_empty() {
            out.push(package);
        }
    }
    let mut result: Vec<String> = Vec::new();
    for kw in out {
        let kw = kw.trim().to_string();
        if kw.is_empty() || result.contains(&kw) {
            continue;
        }
        if IGNORE_STACK_PREFIXES.iter().any(|ig| kw.starts_with(ig)) {
            continue;
        }
        result.push(kw);
    }
    result
}

/// 疑似装了 Mod 才做堆栈分析（PCL: orge/abric/uilt/iteloader 特征）
fn stack_analysis(logs: &Logs, reasons: &mut Vec<CrashReason>) {
    let all = format!(
        "{}{}{}",
        logs.mc.as_deref().unwrap_or(""),
        logs.hs.as_deref().unwrap_or(""),
        logs.crash.as_deref().unwrap_or("")
    );
    if !(all.contains("orge") || all.contains("abric") || all.contains("uilt") || all.contains("iteloader")) {
        return;
    }
    let mut keywords: Vec<String> = Vec::new();
    if let Some(crash) = &logs.crash {
        let head = between(Some(crash), "", "System Details").unwrap_or_default();
        keywords.extend(analyze_stack_keyword(&head));
    }
    if let Some(mc) = &logs.mc {
        let re_fatal = regex::Regex::new(r"/FATAL\] .+?(\r?\n\[)").unwrap();
        for m in re_fatal.find_iter(mc) {
            keywords.extend(analyze_stack_keyword(m.as_str()));
        }
        if let Some(seg) = between(Some(mc), "Unreported exception thrown!", "at oolloo.jlw.Wrapper") {
            keywords.extend(analyze_stack_keyword(&seg));
        }
    }
    if let Some(hs) = &logs.hs {
        if let Some(seg) = between(Some(hs), "T H R E A D", "Registers:") {
            keywords.extend(analyze_stack_keyword(&seg));
        }
    }
    if !keywords.is_empty() {
        match analyze_mod_name(logs, keywords.clone()) {
            Some(names) => push(reasons, "堆栈分析发现Mod名称", names),
            None => push(reasons, "堆栈分析发现关键字", dedup(keywords)),
        }
    }
}

// ---------------- Mod 名反查（PCL AnalyzeModName） ----------------

/// 从崩溃报告/debug.log 的 Mod 行反查实际 Mod 文件名；找不到返回 None
fn analyze_mod_name(logs: &Logs, keywords: Vec<String>) -> Option<Vec<String>> {
    // 预处理关键词：拆括号
    let mut real_keywords: Vec<String> = Vec::new();
    for kw in keywords {
        for sub in kw.split('(') {
            real_keywords.push(sub.trim_matches(|c: char| c == ' ' || c == ')').to_string());
        }
    }

    let mut mod_file_names: Vec<String> = Vec::new();

    // 从崩溃报告获取 Mod 信息
    if let Some(crash) = &logs.crash {
        if crash.contains("A detailed walkthrough of the error") {
            let is_fabric = crash.contains("Fabric Mods");
            let details = match crash.rfind("A detailed walkthrough of the error") {
                Some(pos) => &crash[pos..],
                None => crash.as_str(),
            };
            let details = if is_fabric {
                match details.find("Fabric Mods") {
                    Some(pos) => &details[pos..],
                    None => details,
                }
            } else {
                details
            };
            let details = match details.find('\n') {
                Some(pos) => &details[pos..],
                None => details,
            };
            // 收集含 .jar 的 Mod 行（Forge：含且仅含一个 .jar；Fabric：\t\t 开头且非 fabric 自述行）
            let mut mod_name_lines: Vec<&str> = Vec::new();
            for line in details.lines() {
                let jar_count = line.to_lowercase().matches(".jar").count();
                if jar_count == 1 {
                    mod_name_lines.push(line);
                } else if is_fabric && line.starts_with("\t\t") {
                    let re_fabric_self = regex::Regex::new(r#"\t\tfabric[\w-]*: Fabric"#).unwrap();
                    if !re_fabric_self.is_match(line) {
                        mod_name_lines.push(line);
                    }
                }
            }
            // 关键词匹配行
            let mut hint_lines: Vec<&str> = Vec::new();
            for keyword in &real_keywords {
                let kw = keyword.to_lowercase().replace('_', "");
                for line in &mod_name_lines {
                    let real = line.to_lowercase().replace('_', "");
                    if !real.contains(&kw) {
                        continue;
                    }
                    if real.contains("minecraft.jar") || real.contains(" forge-") || real.contains(" mixin-") {
                        continue;
                    }
                    hint_lines.push(line);
                    break;
                }
            }
            hint_lines.dedup();
            // 从匹配行提取 .jar 文件名
            for line in &hint_lines {
                let name = if is_fabric {
                    regex::Regex::new(r": (.+)").unwrap().captures(line).and_then(|c| c.get(1)).map(|m| {
                        // 去掉最后一个空格分隔的 token（版本号）
                        let t = m.as_str();
                        match t.rfind(' ') {
                            Some(pos) => t[..pos].to_string(),
                            None => t.to_string(),
                        }
                    })
                } else {
                    regex::Regex::new(r"\(([^\t\r\n]+\.jar)\)|(?:\t\t|\| )([^\t|\r\n]+\.jar)")
                        .unwrap()
                        .captures(line)
                        .and_then(|c| c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string()))
                };
                if let Some(n) = name {
                    mod_file_names.push(n);
                }
            }
        }
    }

    // 从 debug.log 获取 Mod 信息（Forge: Found valid mod file xxx.jar with {modid} mods）
    if let Some(debug) = &logs.debug {
        let re_valid = regex::Regex::new(r"valid mod file (.*)").unwrap();
        let mod_lines: Vec<String> = re_valid
            .captures_iter(debug)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
            .collect();
        let mut hint_lines: Vec<&String> = Vec::new();
        for keyword in &real_keywords {
            let tag = format!("{{{keyword}}}");
            for line in &mod_lines {
                if line.contains(&tag) {
                    hint_lines.push(line);
                }
            }
        }
        hint_lines.dedup();
        for line in hint_lines {
            if let Some(pos) = line.find(" with") {
                mod_file_names.push(line[..pos].to_string());
            }
        }
    }

    mod_file_names.dedup();
    if mod_file_names.is_empty() {
        None
    } else {
        Some(mod_file_names)
    }
}

fn try_analyze_mod_name(logs: &Logs, keywords: Vec<String>) -> Vec<String> {
    if keywords.is_empty() {
        return keywords;
    }
    let fallback: Vec<String> = keywords
        .iter()
        .filter(|k| !k.trim().is_empty())
        .cloned()
        .collect();
    analyze_mod_name(logs, keywords).unwrap_or(fallback)
}

// ---------------- Crit3：精准日志匹配（低优先级） ----------------

fn crit3(logs: &Logs, reasons: &mut Vec<CrashReason>) {
    // 游戏日志
    if let Some(mc) = &logs.mc {
        // 极短的程序输出
        if !mc.contains("at net.") && !mc.contains("INFO]") && logs.hs.is_none() && logs.crash.is_none() && mc.len() < 100 {
            push(reasons, "极短的程序输出", vec![mc.clone()]);
        }
        if mc.contains("Mod resolution failed") {
            push(reasons, "Mod加载器报错", vec![]);
        }
        if mc.contains("Failed to create mod instance.") {
            let re1 = regex::Regex::new(r"Failed to create mod instance\. ModID: ([^,\r\n]+)").unwrap();
            let re2 = regex::Regex::new(r"Failed to create mod instance\. ModId ([^\r\n]+?) for ").unwrap();
            let name = seek(&logs.mc, &re1).or_else(|| seek(&logs.mc, &re2)).map(|s| s.trim().to_string());
            let names = try_analyze_mod_name(logs, vec![name.unwrap_or_default()]);
            push(reasons, "Mod初始化失败", names);
        }
    }

    // 崩溃报告
    if let Some(crash) = &logs.crash {
        if crash.contains("\tBlock location: World: ") {
            let re_block = regex::Regex::new(r"\tBlock: Block\{([^}]+)\}").unwrap();
            let re_loc = regex::Regex::new(r"\tBlock location: World: (\([^\)]+\))").unwrap();
            let detail = format!(
                "{} {}",
                seek(&logs.crash, &re_block).unwrap_or_default(),
                seek(&logs.crash, &re_loc).unwrap_or_default()
            );
            push(reasons, "特定方块导致崩溃", vec![detail.trim().to_string()]);
        }
        if crash.contains("\tEntity's Exact location: ") {
            let re_type = regex::Regex::new(r"\tEntity Type: ([^\r\n]+?) \(").unwrap();
            let re_loc = regex::Regex::new(r"\tEntity's Exact location: ([^\r\n]+)").unwrap();
            let detail = format!(
                "{} ({})",
                seek(&logs.crash, &re_type).unwrap_or_default(),
                seek(&logs.crash, &re_loc)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            );
            push(reasons, "特定实体导致崩溃", vec![detail.trim().to_string()]);
        }
    }
}

// ---------------- 工具 ----------------

fn between(text: Option<&str>, start: &str, end: &str) -> Option<String> {
    let text = text?;
    if start.is_empty() {
        let pos = text.find(end)?;
        return Some(text[..pos].to_string());
    }
    let s = text.find(start)? + start.len();
    let rest = &text[s..];
    let e = if end.is_empty() {
        rest.len()
    } else {
        rest.find(end).unwrap_or(rest.len())
    };
    Some(rest[..e].to_string())
}

fn dedup(mut list: Vec<String>) -> Vec<String> {
    list.dedup();
    list
}

// ---------------- 输出 ----------------

/// 单条分析结果的文案（原因 + 细节）
pub fn reason_text(reason: &CrashReason) -> String {
    if reason.details.is_empty() {
        reason.title.clone()
    } else {
        format!("{}（{}）", reason.title, reason.details.join("；"))
    }
}

/// 组装给前端的 JSON：{ reasons: [{title, details}], text: 首要原因文案 }
pub fn to_json(reasons: &[CrashReason]) -> serde_json::Value {
    json!({
        "reasons": reasons.iter().map(|r| json!({
            "title": r.title,
            "details": r.details,
        })).collect::<Vec<_>>(),
        "text": reasons.first().map(reason_text).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oom_in_latest_log_is_detected() {
        let logs = Logs {
            mc: Some("java.lang.OutOfMemoryError: Java heap space\n\tat x".into()),
            hs: None,
            crash: None,
            debug: None,
        };
        let reasons = analyze_logs(&logs);
        assert!(reasons.iter().any(|r| r.title == "内存不足"));
    }

    #[test]
    fn fabric_mixin_failure_names_the_mod() {
        let crash = "\t\tjei: Just Enough Items 15.0.0.28 jei-1.20.1-forge-15.0.0.28.jar\n\
                     MixinApplyError: Error initialising mixin 'net.x' from mod jei] from mod jei failed\n";
        let logs = Logs {
            mc: None,
            hs: None,
            crash: Some(crash.into()),
            debug: None,
        };
        let reasons = analyze_logs(&logs);
        assert!(reasons.iter().any(|r| r.title == "ModMixin失败"), "{:?}", reasons);
    }

    #[test]
    fn optifine_forge_conflict_is_matched() {
        let logs = Logs {
            mc: Some("java.lang.NoSuchMethodError: 'void net.minecraft.client.renderer.block.model.BakedQuad.<init>".into()),
            hs: None,
            crash: None,
            debug: None,
        };
        let reasons = analyze_logs(&logs);
        assert!(reasons.iter().any(|r| r.title == "OptiFine与Forge不兼容"));
    }

    #[test]
    fn hs_err_intel_driver_is_detected() {
        let logs = Logs {
            mc: None,
            hs: Some("EXCEPTION_ACCESS_VIOLATION (0xc0000005)\n# C  [ig4icd64.dll+0x]".into()),
            crash: None,
            debug: None,
        };
        let reasons = analyze_logs(&logs);
        assert!(reasons.iter().any(|r| r.title.contains("Intel驱动")));
    }

    #[test]
    fn stack_keyword_extracts_mod_package() {
        let stack = "\tat com.example.coolmod.CoolHandler.tick(CoolHandler.java:42)\n\tat java.base/java.lang.Thread.run(Thread.java:833)\n";
        let kws = analyze_stack_keyword(stack);
        assert!(kws.iter().any(|k| k.contains("coolmod")), "{:?}", kws);
        assert!(!kws.iter().any(|k| k.starts_with("java")), "{:?}", kws);
    }

    #[test]
    fn empty_logs_report_no_files() {
        let reasons = analyze_logs(&Logs { mc: None, hs: None, crash: None, debug: None });
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].title, REASON_NONE);
    }
}
