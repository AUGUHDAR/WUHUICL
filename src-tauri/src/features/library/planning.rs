//! Pure dependency and version compatibility helpers shared by library planning.

/// Compare launcher mod versions after removing build metadata suffixes.
pub(crate) fn compare_mod_version(a: &str, b: &str) -> std::cmp::Ordering {
    let norm = |s: &str| -> Vec<i64> {
        let stripped = regex::Regex::new(r"[+_].*$")
            .unwrap()
            .replace(s, "")
            .to_string();
        stripped
            .split(['.', '-'])
            .map(|x| x.trim().parse::<i64>().unwrap_or(0))
            .collect()
    };
    let pa = norm(a);
    let pb = norm(b);
    for i in 0..pa.len().max(pb.len()) {
        let d = pa.get(i).copied().unwrap_or(0) - pb.get(i).copied().unwrap_or(0);
        if d != 0 {
            return d.cmp(&0);
        }
    }
    std::cmp::Ordering::Equal
}

pub(crate) fn compare_version_desc_impl(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering as O;
    let split = |s: &str| -> Vec<String> { s.split(['.', '-']).map(str::to_string).collect() };
    let (pa, pb) = (split(a), split(b));
    for i in 0..pa.len().max(pb.len()) {
        match (pa.get(i), pb.get(i)) {
            (None, _) => return O::Greater,
            (_, None) => return O::Less,
            (Some(xa), Some(xb)) => {
                let a_num = !xa.is_empty() && xa.chars().all(|c| c.is_ascii_digit());
                let b_num = !xb.is_empty() && xb.chars().all(|c| c.is_ascii_digit());
                if a_num && b_num {
                    let d = xb
                        .parse::<u64>()
                        .unwrap_or(0)
                        .cmp(&xa.parse::<u64>().unwrap_or(0));
                    if d != O::Equal {
                        return d;
                    }
                } else if a_num {
                    return O::Less;
                } else if b_num {
                    return O::Greater;
                } else {
                    let c = xb.cmp(xa);
                    if c != O::Equal {
                        return c;
                    }
                }
            }
        }
    }
    O::Equal
}

/// Compare Minecraft loader versions in descending preference order.
pub fn compare_version_desc(a: &str, b: &str) -> std::cmp::Ordering {
    compare_version_desc_impl(a, b)
}
