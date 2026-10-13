//! 对应原版 core/typedNbt.ts 的无损 NBT 编解码层。

use flate2::read::{MultiGzDecoder, ZlibDecoder};
use flate2::write::GzEncoder;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashSet;
use std::io::{Read, Write};

pub const TAG_END: u8 = 0;
pub const TAG_BYTE: u8 = 1;
pub const TAG_SHORT: u8 = 2;
pub const TAG_INT: u8 = 3;
pub const TAG_LONG: u8 = 4;
pub const TAG_FLOAT: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_BYTE_ARRAY: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_LIST: u8 = 9;
pub const TAG_COMPOUND: u8 = 10;
pub const TAG_INT_ARRAY: u8 = 11;
pub const TAG_LONG_ARRAY: u8 = 12;

const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_TAKE: usize = 16 * 1024 * 1024;
const MAX_COLLECTION: i32 = 16_777_216;
const MAX_NODES: usize = 2_000_000;
const MAX_DEPTH: u32 = 64;

const ERR_TRUNCATED: &str = "NBT 数据截断";
const ERR_TOO_DEEP: &str = "NBT 结构过深或标签过多";
const ERR_COUNT: &str = "NBT 集合长度超限";
const ERR_LIST_TYPE: &str = "NBT 列表类型无效";
const ERR_DUP_KEY: &str = "NBT 标签重复";
const ERR_BROKEN_UTF: &str = "NBT UTF 字符串损坏";
const ERR_INVALID_UTF: &str = "NBT Modified UTF-8 编码无效";
const INVALID_TYPE: &str = "NBT 标签类型无效";

#[derive(Debug, Clone, PartialEq)]
pub enum NbtPayload {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NbtTag {
    pub tag_type: u8,
    pub value: NbtPayload,
    pub element_type: Option<u8>,
}

impl NbtPayload {
    pub fn tag_type(&self) -> u8 {
        match self {
            NbtPayload::Byte(_) => TAG_BYTE,
            NbtPayload::Short(_) => TAG_SHORT,
            NbtPayload::Int(_) => TAG_INT,
            NbtPayload::Long(_) => TAG_LONG,
            NbtPayload::Float(_) => TAG_FLOAT,
            NbtPayload::Double(_) => TAG_DOUBLE,
            NbtPayload::ByteArray(_) => TAG_BYTE_ARRAY,
            NbtPayload::String(_) => TAG_STRING,
            NbtPayload::List(_) => TAG_LIST,
            NbtPayload::Compound(_) => TAG_COMPOUND,
            NbtPayload::IntArray(_) => TAG_INT_ARRAY,
            NbtPayload::LongArray(_) => TAG_LONG_ARRAY,
        }
    }

    fn num_i64(&self) -> Option<i64> {
        match self {
            NbtPayload::Byte(v) => Some(*v as i64),
            NbtPayload::Short(v) => Some(*v as i64),
            NbtPayload::Int(v) => Some(*v as i64),
            NbtPayload::Long(v) => Some(*v),
            NbtPayload::Float(v) => Some(*v as i64),
            NbtPayload::Double(v) => Some(*v as i64),
            _ => None,
        }
    }

    fn num_f64(&self) -> Option<f64> {
        match self {
            NbtPayload::Byte(v) => Some(*v as f64),
            NbtPayload::Short(v) => Some(*v as f64),
            NbtPayload::Int(v) => Some(*v as f64),
            NbtPayload::Long(v) => Some(*v as f64),
            NbtPayload::Float(v) => Some(*v as f64),
            NbtPayload::Double(v) => Some(*v),
            _ => None,
        }
    }
}

impl NbtTag {
    pub fn byte(&self) -> Option<i8> {
        self.value.num_i64().map(|v| v as i8)
    }
    pub fn short(&self) -> Option<i16> {
        self.value.num_i64().map(|v| v as i16)
    }
    pub fn int(&self) -> Option<i32> {
        self.value.num_i64().map(|v| v as i32)
    }
    pub fn long(&self) -> Option<i64> {
        self.value.num_i64()
    }
    pub fn float(&self) -> Option<f32> {
        self.value.num_f64().map(|v| v as f32)
    }
    pub fn double(&self) -> Option<f64> {
        self.value.num_f64()
    }
    pub fn str(&self) -> Option<&str> {
        match &self.value {
            NbtPayload::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.value {
            NbtPayload::ByteArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn items(&self) -> Option<&[NbtTag]> {
        match &self.value {
            NbtPayload::List(v) => Some(v),
            _ => None,
        }
    }
    pub fn entries(&self) -> Option<&[(String, NbtTag)]> {
        match &self.value {
            NbtPayload::Compound(v) => Some(v),
            _ => None,
        }
    }
    pub fn ints(&self) -> Option<&[i32]> {
        match &self.value {
            NbtPayload::IntArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn longs(&self) -> Option<&[i64]> {
        match &self.value {
            NbtPayload::LongArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&NbtTag> {
        self.entries()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
    pub fn to_json(&self) -> Value {
        match &self.value {
            NbtPayload::Byte(v) => Value::from(*v as i64),
            NbtPayload::Short(v) => Value::from(*v as i64),
            NbtPayload::Int(v) => Value::from(*v as i64),
            NbtPayload::Long(v) => Value::from(*v),
            NbtPayload::Float(v) => json_float(*v as f64),
            NbtPayload::Double(v) => json_float(*v),
            NbtPayload::ByteArray(v) => Value::Array(v.iter().map(|b| Value::from(*b as i64)).collect()),
            NbtPayload::String(v) => Value::String(v.clone()),
            NbtPayload::List(v) => Value::Array(v.iter().map(|t| t.to_json()).collect()),
            NbtPayload::Compound(v) => {
                let mut map = serde_json::Map::new();
                for (k, t) in v {
                    map.insert(k.clone(), t.to_json());
                }
                Value::Object(map)
            }
            NbtPayload::IntArray(v) => Value::Array(v.iter().map(|n| Value::from(*n as i64)).collect()),
            NbtPayload::LongArray(v) => Value::Array(v.iter().map(|n| Value::from(*n)).collect()),
        }
    }
}

fn json_float(v: f64) -> Value {
    if v.is_nan() || v.is_infinite() {
        return Value::Null;
    }
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

pub fn tag(type_: u8, value: NbtPayload, element_type: Option<u8>) -> NbtTag {
    NbtTag {
        tag_type: type_,
        value,
        element_type,
    }
}

pub fn compound(pairs: Vec<(String, NbtTag)>) -> NbtTag {
    tag(TAG_COMPOUND, NbtPayload::Compound(pairs), None)
}
pub fn byte(v: i8) -> NbtTag {
    tag(TAG_BYTE, NbtPayload::Byte(v), None)
}
pub fn short(v: i16) -> NbtTag {
    tag(TAG_SHORT, NbtPayload::Short(v), None)
}
pub fn int(v: i32) -> NbtTag {
    tag(TAG_INT, NbtPayload::Int(v), None)
}
pub fn long(v: i64) -> NbtTag {
    tag(TAG_LONG, NbtPayload::Long(v), None)
}
pub fn float_(v: f32) -> NbtTag {
    tag(TAG_FLOAT, NbtPayload::Float(v), None)
}
pub fn double(v: f64) -> NbtTag {
    tag(TAG_DOUBLE, NbtPayload::Double(v), None)
}
pub fn byte_array(v: Vec<u8>) -> NbtTag {
    tag(TAG_BYTE_ARRAY, NbtPayload::ByteArray(v), None)
}
pub fn string(v: impl Into<String>) -> NbtTag {
    tag(TAG_STRING, NbtPayload::String(v.into()), None)
}
pub fn list(values: Vec<NbtTag>, element_type: u8) -> NbtTag {
    tag(TAG_LIST, NbtPayload::List(values), Some(element_type))
}
pub fn int_array(v: Vec<i32>) -> NbtTag {
    tag(TAG_INT_ARRAY, NbtPayload::IntArray(v), None)
}
pub fn long_array(v: Vec<i64>) -> NbtTag {
    tag(TAG_LONG_ARRAY, NbtPayload::LongArray(v), None)
}

pub fn nv(i: i64) -> NbtPayload {
    NbtPayload::Int(i as i32)
}
pub fn nl(i: i64) -> NbtPayload {
    NbtPayload::Long(i)
}
pub fn nd(f: f64) -> NbtPayload {
    NbtPayload::Double(f)
}
pub fn nstr(v: impl Into<String>) -> NbtPayload {
    NbtPayload::String(v.into())
}
pub fn nbytes(v: Vec<u8>) -> NbtPayload {
    NbtPayload::ByteArray(v)
}

pub fn payload_from_json(v: &Value) -> Option<NbtPayload> {
    match v {
        Value::Bool(b) => Some(NbtPayload::Byte(if *b { 1 } else { 0 })),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= i32::MIN as i64 && i <= i32::MAX as i64 {
                    return Some(nv(i));
                }
                return Some(NbtPayload::Long(i));
            }
            if let Some(u) = n.as_u64() {
                return Some(NbtPayload::Long(u as i64));
            }
            n.as_f64().map(nd)
        }
        Value::String(s) => Some(NbtPayload::String(s.clone())),
        _ => None,
    }
}

// 对应原版 decodeNbtString，三字节代理位放行。
pub fn decode_nbt_string(bytes: &[u8]) -> Result<String, String> {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len().min(MAX_TAKE));
    let mut p = 0usize;
    while p < bytes.len() {
        let a = bytes[p];
        p += 1;
        if a < 0x80 {
            units.push(a as u16);
            continue;
        }
        if a & 0xe0 == 0xc0 {
            if p >= bytes.len() {
                return Err(ERR_BROKEN_UTF.into());
            }
            let b = bytes[p];
            p += 1;
            if b & 0xc0 != 0x80 {
                return Err(ERR_BROKEN_UTF.into());
            }
            let n = (((a as u32) & 31) << 6) | ((b as u32) & 63);
            if n != 0 && n < 128 {
                return Err(ERR_BROKEN_UTF.into());
            }
            units.push(n as u16);
        } else if a & 0xf0 == 0xe0 {
            if p + 1 >= bytes.len() {
                return Err(ERR_BROKEN_UTF.into());
            }
            let b = bytes[p];
            let c = bytes[p + 1];
            p += 2;
            if b & 0xc0 != 0x80 || c & 0xc0 != 0x80 {
                return Err(ERR_BROKEN_UTF.into());
            }
            let n = (((a as u32) & 15) << 12) | (((b as u32) & 63) << 6) | ((c as u32) & 63);
            if n < 2048 {
                return Err(ERR_BROKEN_UTF.into());
            }
            units.push(n as u16);
        } else {
            return Err(ERR_INVALID_UTF.into());
        }
    }
    Ok(string_from_units(&units))
}

pub fn encode_nbt_string(value: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::with_capacity(value.len());
    for unit in value.encode_utf16() {
        let c = unit as u32;
        if c > 0 && c < 128 {
            bytes.push(c as u8);
        } else if c < 2048 {
            bytes.push(0xc0 | (c >> 6) as u8);
            bytes.push(0x80 | (c & 63) as u8);
        } else {
            bytes.push(0xe0 | (c >> 12) as u8);
            bytes.push(0x80 | ((c >> 6) & 63) as u8);
            bytes.push(0x80 | (c & 63) as u8);
        }
    }
    bytes
}

fn string_from_units(units: &[u16]) -> String {
    let mut out = String::with_capacity(units.len());
    let mut i = 0usize;
    while i < units.len() {
        let u = units[i];
        let (ch, step) = if (0xd800..0xdc00).contains(&u)
            && i + 1 < units.len()
            && (0xdc00..0xe000).contains(&units[i + 1])
        {
            let pair = 0x1_0000 + (((u as u32 - 0xd800) << 10) | (units[i + 1] as u32 - 0xdc00));
            (
                char::from_u32(pair).unwrap_or('\u{fffd}'),
                2usize,
            )
        } else {
            (
                char::from_u32(u as u32).unwrap_or('\u{fffd}'),
                1usize,
            )
        };
        out.push(ch);
        i += step;
    }
    out
}

pub fn read_typed_nbt(input: &[u8]) -> Result<(String, NbtTag), String> {
    let buffer = decompress_input(input)?;
    if buffer.len() > MAX_BYTES {
        return Err("NBT 解压后超过 128 MB".into());
    }
    let mut r = Reader {
        b: &buffer,
        p: 0,
        nodes: 0,
    };
    let type_ = r.u8()?;
    if type_ != TAG_COMPOUND {
        return Err("投影 NBT 根必须是 Compound".into());
    }
    let name = r.string()?;
    let root = r.read(TAG_COMPOUND, 0)?;
    if r.p != buffer.len() {
        return Err("NBT 存在额外尾数据".into());
    }
    Ok((name, root))
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
    nodes: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > MAX_TAKE {
            return Err(ERR_TRUNCATED.into());
        }
        let end = match self.p.checked_add(n) {
            Some(v) => v,
            None => return Err(ERR_TRUNCATED.into()),
        };
        if end > self.b.len() {
            return Err(ERR_TRUNCATED.into());
        }
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> Result<i8, String> {
        Ok(self.take(1)?[0] as i8)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        let s = self.take(4)?;
        Ok(i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i64(&mut self) -> Result<i64, String> {
        let s = self.take(8)?;
        Ok(i64::from_be_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        let s = self.take(4)?;
        Ok(f32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f64(&mut self) -> Result<f64, String> {
        let s = self.take(8)?;
        Ok(f64::from_be_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
        ]))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        let bytes = self.take(n)?;
        decode_nbt_string(bytes)
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.i32()?;
        if n < 0 || n > MAX_COLLECTION {
            return Err(ERR_COUNT.into());
        }
        Ok(n as usize)
    }
    fn read(&mut self, type_: u8, depth: u32) -> Result<NbtTag, String> {
        if depth > MAX_DEPTH {
            return Err(ERR_TOO_DEEP.into());
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ERR_TOO_DEEP.into());
        }
        let mut element_type: Option<u8> = None;
        let value = match type_ {
            TAG_BYTE => NbtPayload::Byte(self.i8()?),
            TAG_SHORT => NbtPayload::Short(self.u16()? as i16),
            TAG_INT => NbtPayload::Int(self.i32()?),
            TAG_LONG => NbtPayload::Long(self.i64()?),
            TAG_FLOAT => NbtPayload::Float(self.f32()?),
            TAG_DOUBLE => NbtPayload::Double(self.f64()?),
            TAG_BYTE_ARRAY => {
                let n = self.count()?;
                NbtPayload::ByteArray(self.take(n)?.to_vec())
            }
            TAG_STRING => NbtPayload::String(self.string()?),
            TAG_LIST => {
                let element = self.u8()?;
                let n = self.count()?;
                if element > 12 || (element == 0 && n != 0) {
                    return Err(ERR_LIST_TYPE.into());
                }
                let mut items = Vec::with_capacity(n.min(65_536));
                for _ in 0..n {
                    items.push(self.read(element, depth + 1)?);
                }
                element_type = Some(element);
                NbtPayload::List(items)
            }
            TAG_COMPOUND => {
                let mut entries: Vec<(String, NbtTag)> = Vec::new();
                let mut seen: HashSet<String> = HashSet::new();
                loop {
                    let child = self.u8()?;
                    if child == TAG_END {
                        break;
                    }
                    let name = self.string()?;
                    if !seen.insert(name.clone()) {
                        return Err(ERR_DUP_KEY.into());
                    }
                    let child_tag = self.read(child, depth + 1)?;
                    entries.push((name, child_tag));
                }
                NbtPayload::Compound(entries)
            }
            TAG_INT_ARRAY => {
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(65_536));
                for _ in 0..n {
                    out.push(self.i32()?);
                }
                NbtPayload::IntArray(out)
            }
            TAG_LONG_ARRAY => {
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(65_536));
                for _ in 0..n {
                    out.push(self.i64()?);
                }
                NbtPayload::LongArray(out)
            }
            other => return Err(format!("NBT 标签类型无效：{}", other)),
        };
        Ok(tag(type_, value, element_type))
    }
}

pub fn canonical(t: &NbtTag) -> NbtTag {
    match &t.value {
        NbtPayload::Compound(entries) => {
            let mut out: Vec<(String, NbtTag)> = entries
                .iter()
                .map(|(k, v)| (k.clone(), canonical(v)))
                .collect();
            out.sort_by(|a, b| a.0.cmp(&b.0));
            tag(TAG_COMPOUND, NbtPayload::Compound(out), None)
        }
        NbtPayload::List(items) => tag(
            TAG_LIST,
            NbtPayload::List(items.iter().map(canonical).collect()),
            t.element_type,
        ),
        other => tag(t.tag_type, other.clone(), t.element_type),
    }
}

/// 对应原版 writeTypedNbt，compressed 默认 true。
pub fn write_typed_nbt(root: &NbtTag, name: &str, compressed: bool) -> Result<Vec<u8>, String> {
    if root.tag_type != TAG_COMPOUND {
        return Err("NBT 根必须是 Compound".into());
    }
    let mut w = Writer {
        out: Vec::new(),
        nodes: 0,
    };
    w.u8(TAG_COMPOUND)?;
    w.text(name)?;
    w.write(root, 0)?;
    if compressed {
        gzip(&w.out)
    } else {
        Ok(w.out)
    }
}

/// 等价 writeTypedNbt(tag(10,{value:t}),'',false)。
pub fn write_tag_anonymous(t: &NbtTag) -> Result<Vec<u8>, String> {
    write_typed_nbt(&compound(vec![("value".to_string(), t.clone())]), "", false)
}

struct Writer {
    out: Vec<u8>,
    nodes: usize,
}

impl Writer {
    fn push(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.out.extend_from_slice(bytes);
        if self.out.len() > MAX_BYTES {
            return Err("NBT 写入超过 128 MB".into());
        }
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), String> {
        self.push(&[v])
    }
    fn i32b(&mut self, v: i32) -> Result<(), String> {
        self.push(&v.to_be_bytes())
    }
    fn text(&mut self, s: &str) -> Result<(), String> {
        let bytes = encode_nbt_string(s);
        if bytes.len() > 65_535 {
            return Err("NBT 字符串过长".into());
        }
        self.push(&(bytes.len() as u16).to_be_bytes())?;
        self.push(&bytes)
    }
    fn write(&mut self, t: &NbtTag, depth: u32) -> Result<(), String> {
        if depth > MAX_DEPTH {
            return Err(ERR_TOO_DEEP.into());
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ERR_TOO_DEEP.into());
        }
        // 写出按 tag_type 截断，不新增文案。
        match t.tag_type {
            TAG_BYTE => {
                let v = t.value.num_i64().ok_or(INVALID_TYPE)?;
                self.push(&(v as i8).to_be_bytes())?;
            }
            TAG_SHORT => {
                let v = t.value.num_i64().ok_or(INVALID_TYPE)?;
                self.push(&(v as i16).to_be_bytes())?;
            }
            TAG_INT => {
                let v = t.value.num_i64().ok_or(INVALID_TYPE)?;
                self.i32b(v as i32)?;
            }
            TAG_LONG => {
                let v = t.value.num_i64().ok_or(INVALID_TYPE)?;
                self.push(&v.to_be_bytes())?;
            }
            TAG_FLOAT => {
                let v = t.value.num_f64().ok_or(INVALID_TYPE)?;
                self.push(&(v as f32).to_be_bytes())?;
            }
            TAG_DOUBLE => {
                let v = t.value.num_f64().ok_or(INVALID_TYPE)?;
                self.push(&v.to_be_bytes())?;
            }
            TAG_BYTE_ARRAY => {
                let bytes = match &t.value {
                    NbtPayload::ByteArray(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                self.i32b(bytes.len() as i32)?;
                self.push(bytes)?;
            }
            TAG_STRING => {
                let s = match &t.value {
                    NbtPayload::String(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                self.text(s)?;
            }
            TAG_LIST => {
                let items = match &t.value {
                    NbtPayload::List(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                let element = match t.element_type {
                    Some(v) => v,
                    None => match items.first() {
                        Some(first) => first.tag_type,
                        None => TAG_END,
                    },
                };
                self.u8(element)?;
                self.i32b(items.len() as i32)?;
                for item in items {
                    if item.tag_type != element {
                        return Err("NBT 列表不是同类型".into());
                    }
                    self.write(item, depth + 1)?;
                }
            }
            TAG_COMPOUND => {
                let entries = match &t.value {
                    NbtPayload::Compound(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                for (key, child) in entries {
                    self.u8(child.tag_type)?;
                    self.text(key)?;
                    self.write(child, depth + 1)?;
                }
                self.u8(TAG_END)?;
            }
            TAG_INT_ARRAY => {
                let values = match &t.value {
                    NbtPayload::IntArray(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                self.i32b(values.len() as i32)?;
                for v in values {
                    self.i32b(*v as i32)?;
                }
            }
            TAG_LONG_ARRAY => {
                let values = match &t.value {
                    NbtPayload::LongArray(v) => v,
                    _ => return Err(INVALID_TYPE.into()),
                };
                self.i32b(values.len() as i32)?;
                for v in values {
                    self.push(&v.to_be_bytes())?;
                }
            }
            _ => return Err(INVALID_TYPE.into()),
        }
        Ok(())
    }
}

pub fn read_gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    decode_gzip(bytes, u64::MAX)
}

pub fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(bytes)
        .map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

fn decode_gzip(bytes: &[u8], cap: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    MultiGzDecoder::new(bytes)
        .take(cap)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn decompress_input(input: &[u8]) -> Result<Cow<'_, [u8]>, String> {
    if input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
        return Ok(Cow::Owned(decode_gzip(input, MAX_BYTES as u64 + 1)?));
    }
    if !input.is_empty() && input[0] == 0x78 {
        let mut out = Vec::new();
        ZlibDecoder::new(input)
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| e.to_string())?;
        return Ok(Cow::Owned(out));
    }
    Ok(Cow::Borrowed(input))
}
