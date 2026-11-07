//! 离线 MC 百科词典（数据源自 mcmod.cn，经 HMCL 的 mod_data.txt/modpack_data.txt 管线，
//! GPL-3.0，见 THIRD_PARTY_NOTICES）。PCL2 / HMCL / 美西螈三家启动器的共识路线：
//! 离线词典做「中文名 → 英文身份」桥接，实时百科抓取只做词典未命中时的兜底。
//!
//! 行格式（六字段分号分隔，字段可空）：`curseforgeSlug;mcmodId;modid,modid;中文名;英文原名;缩写`

use std::collections::HashMap;
use std::sync::OnceLock;

pub struct WikiEntry {
    /// CurseForge slug（可空；Modrinth 侧用英文名匹配，不依赖 slug 体系）
    pub cf_slug: String,
    /// mcmod.cn class 页 id（0 = 无；百科链接直达 class/{id}.html）
    pub mcmod_id: u32,
    pub mod_ids: Vec<String>,
    /// 中文名
    pub zh: String,
    /// 英文原名
    pub subname: String,
    /// 英文缩写（如 JEI）
    pub abbr: String,
}

const MOD_DATA: &str = include_str!("../assets/mod_data.txt");
const MODPACK_DATA: &str = include_str!("../assets/modpack_data.txt");

/// 作者自有项目的私货条目（暂未收录于 mcmod.cn），解析后并入词典：
/// 沃克联机（VoxLink）与沃克键盘（VoxKbd），字段格式与上游一致。
const EXTRA_ENTRIES: &[&str] = &[";0;;沃克联机;VoxLink;", ";0;;沃克键盘;VoxKbd;"];

struct WikiDb {
    entries: Vec<WikiEntry>,
    /// (关键词, 条目下标)：中文名 / 英文原名 / 缩写各一条，LCS 搜索用
    keywords: Vec<(String, usize)>,
    max_keyword_len: usize,
    /// 清洗后的英文原名 → 下标（结果标题中文化 + 百科 id 附加用）
    by_subname: HashMap<String, usize>,
    /// CF slug（小写）→ 下标
    by_cf_slug: HashMap<String, usize>,
}

fn clean_subname(s: &str) -> String {
    // HMCL cleanSubname 口径：仅保留字母数字与 . + \，剔除空白/标点/emoji
    s.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '+' | '\\'))
        .collect::<String>()
        .to_lowercase()
}

fn parse_line(line: &str) -> Option<WikiEntry> {
    let items: Vec<&str> = line.split(';').collect();
    if items.len() != 6 {
        return None;
    }
    Some(WikiEntry {
        cf_slug: items[0].trim().to_string(),
        mcmod_id: items[1].trim().parse().unwrap_or(0),
        mod_ids: items[2]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        zh: items[3].trim().to_string(),
        subname: items[4].trim().to_string(),
        abbr: items[5].trim().to_string(),
    })
}

fn build_db(text: &str) -> (Vec<WikiEntry>, Vec<(String, usize)>, usize, HashMap<String, usize>, HashMap<String, usize>) {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(e) = parse_line(line) {
            entries.push(e);
        }
    }
    let mut keywords: Vec<(String, usize)> = Vec::new();
    let mut by_subname: HashMap<String, usize> = HashMap::new();
    let mut by_cf_slug: HashMap<String, usize> = HashMap::new();
    let mut max_keyword_len = 0usize;
    for (i, e) in entries.iter().enumerate() {
        for key in [&e.zh, &e.subname, &e.abbr] {
            let key = key.trim().to_lowercase();
            if key.is_empty() {
                continue;
            }
            max_keyword_len = max_keyword_len.max(key.chars().count());
            keywords.push((key, i));
        }
        if !e.subname.is_empty() {
            by_subname.insert(clean_subname(&e.subname), i);
        }
        if !e.cf_slug.is_empty() {
            by_cf_slug.insert(e.cf_slug.to_lowercase(), i);
        }
    }
    (entries, keywords, max_keyword_len, by_subname, by_cf_slug)
}

fn db() -> &'static WikiDb {
    static DB: OnceLock<WikiDb> = OnceLock::new();
    DB.get_or_init(|| {
        let (mut entries, mut keywords, mut max_len, mut by_subname, mut by_cf_slug) =
            build_db(MOD_DATA);
        let (p_entries, p_keywords, p_max, p_subname, p_slug) = build_db(MODPACK_DATA);
        let base = entries.len();
        entries.extend(p_entries);
        keywords.extend(p_keywords.into_iter().map(|(k, i)| (k, i + base)));
        max_len = max_len.max(p_max);
        for (k, i) in p_subname {
            by_subname.entry(k).or_insert(i + base);
        }
        for (k, i) in p_slug {
            by_cf_slug.entry(k).or_insert(i + base);
        }
        for line in EXTRA_ENTRIES {
            if let Some(e) = parse_line(line) {
                let i = entries.len();
                for key in [&e.zh, &e.subname, &e.abbr] {
                    let key = key.trim().to_lowercase();
                    if !key.is_empty() {
                        max_len = max_len.max(key.chars().count());
                        keywords.push((key, i));
                    }
                }
                if !e.subname.is_empty() {
                    by_subname.entry(clean_subname(&e.subname)).or_insert(i);
                }
                entries.push(e);
            }
        }
        WikiDb { entries, keywords, max_keyword_len: max_len, by_subname, by_cf_slug }
    })
}

/// 最长公共子序列长度（HMCL searchMod 同款算法：容忍漏字/别名的模糊匹配）
fn lcs_len(a: &[char], b: &[char]) -> usize {
    let (m, n) = (a.len(), b.len());
    if m == 0 || n == 0 {
        return 0;
    }
    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        for j in 1..=n {
            curr[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|v| *v = 0);
    }
    prev[n]
}

pub struct WikiMatch<'a> {
    pub entry: &'a WikiEntry,
    /// LCS 命中长度（降序已排）
    pub score: usize,
}

/// 中文/别名模糊搜索：LCS ≥ max(1, 词长-3) 即命中（HMCL searchMod 阈值），按 LCS 降序取前 max 条
pub fn search_wiki(query: &str, max: usize) -> Vec<WikiMatch<'static>> {
    let key: String = query.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    if key.is_empty() || max == 0 {
        return Vec::new();
    }
    let db = db();
    let chars: Vec<char> = key.chars().collect();
    let threshold = std::cmp::max(1, chars.len().saturating_sub(3));
    let mut hits: Vec<(usize, usize)> = db
        .keywords
        .iter()
        .filter_map(|(keyword, idx)| {
            let kw: Vec<char> = keyword.chars().collect();
            let value = lcs_len(&chars, &kw);
            (value >= threshold).then_some((value, *idx))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<WikiMatch<'static>> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (value, idx) in hits {
        if seen.insert(idx) {
            let entry: &'static WikiEntry = &db.entries[idx];
            out.push(WikiMatch { entry, score: value });
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// 按英文原名精确查（结果标题中文化 + 百科 id 附加；HMCL 本地模组页同款入口）
pub fn lookup_by_name(english_name: &str) -> Option<&'static WikiEntry> {
    let key = clean_subname(english_name);
    if key.is_empty() {
        return None;
    }
    let db = db();
    db.by_subname.get(&key).map(|&i| &db.entries[i])
}

/// 按 CurseForge slug 查
pub fn lookup_by_cf_slug(slug: &str) -> Option<&'static WikiEntry> {
    let key = slug.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let db = db();
    db.by_cf_slug.get(&key).map(|&i| &db.entries[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_loads_and_privates_present() {
        let hits = search_wiki("工业时代", 5);
        assert!(hits.iter().any(|m| m.entry.subname.eq_ignore_ascii_case("Industrial Craft 2")));
        let vox = search_wiki("沃克联机", 3);
        assert!(vox.iter().any(|m| m.entry.subname == "VoxLink"));
        let kbd = search_wiki("沃克键盘", 3);
        assert!(kbd.iter().any(|m| m.entry.subname == "VoxKbd"));
    }

    #[test]
    fn lcs_fuzzy_tolerates_missing_tail() {
        // 漏尾字也能命中（LCS ≥ 词长-3）
        assert!(!search_wiki("工业时", 3).is_empty());
        assert!(search_wiki("zzzz不存在的模组名称", 3).is_empty());
    }

    #[test]
    fn lookup_by_name_and_slug() {
        assert_eq!(lookup_by_name("Industrial Craft 2").unwrap().zh, "工业时代2");
        assert_eq!(lookup_by_cf_slug("industrial-craft").unwrap().mcmod_id, 2);
        assert!(lookup_by_name("").is_none());
    }
}
