//! NBT（Named Binary Tag）读写器（对应原版 core/nbt.ts 1:1）。
//! 读取覆盖标准 0..12 标签并自动识别 gzip/zlib；写入器只暴露启动器当前
//! 需要的 servers.dat 子集（NbtValue::List/Compound/String/Int/Byte/Long）。

use flate2::read::{GzDecoder, ZlibDecoder};
use serde_json::{json, Value};
use std::io::Read;

const MAX_DECOMPRESSED_NBT: usize = 64 * 1024 * 1024;

/// 列表值（写入侧需要显式 item type）
#[derive(Debug, Clone)]
pub struct NbtList {
    pub item_type: u8,
    pub values: Vec<NbtValue>,
}

/// NBT 值模型（读取后转 Value；写入用本枚举）
#[derive(Debug, Clone)]
pub enum NbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(NbtList),
    Compound(Vec<(String, NbtValue)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl NbtValue {
    pub fn tag_type(&self) -> u8 {
        match self {
            NbtValue::Byte(_) => 1,
            NbtValue::Short(_) => 2,
            NbtValue::Int(_) => 3,
            NbtValue::Long(_) => 4,
            NbtValue::Float(_) => 5,
            NbtValue::Double(_) => 6,
            NbtValue::ByteArray(_) => 7,
            NbtValue::String(_) => 8,
            NbtValue::List(_) => 9,
            NbtValue::Compound(_) => 10,
            NbtValue::IntArray(_) => 11,
            NbtValue::LongArray(_) => 12,
        }
    }
}

// ---------------- 读 ----------------

struct Reader<'a> {
    buf: &'a [u8],
    off: usize,
}

impl<'a> Reader<'a> {
    fn ensure(&self, bytes: usize) -> Result<(), String> {
        if self.off + bytes > self.buf.len() {
            return Err("NBT 数据被截断或长度无效".into());
        }
        Ok(())
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        self.ensure(n)?;
        let s = &self.buf[self.off..self.off + n];
        self.off += n;
        Ok(s)
    }
    fn ty(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn byte(&mut self) -> Result<i8, String> {
        Ok(self.take(1)?[0] as i8)
    }
    fn short(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn unsigned_short(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn int(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn long(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn float(&mut self) -> Result<f32, String> {
        Ok(f32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn double(&mut self) -> Result<f64, String> {
        Ok(f64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let len = self.unsigned_short()? as usize;
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).to_string())
    }
    fn collection_length(&self) -> Result<usize, String> {
        // 长度在调用方读（int），这里只做上限校验
        Ok(0)
    }

    fn payload(&mut self, ty: u8, depth: u32) -> Result<NbtValue, String> {
        if depth > 64 {
            return Err("NBT 嵌套层级过深".into());
        }
        Ok(match ty {
            1 => NbtValue::Byte(self.byte()?),
            2 => NbtValue::Short(self.short()?),
            3 => NbtValue::Int(self.int()?),
            4 => NbtValue::Long(self.long()?),
            5 => NbtValue::Float(self.float()?),
            6 => NbtValue::Double(self.double()?),
            7 => {
                let length = self.int()?;
                if length < 0 {
                    return Err(format!("NBT 集合长度异常: {length}"));
                }
                NbtValue::ByteArray(self.take(length as usize)?.to_vec())
            }
            8 => NbtValue::String(self.string()?),
            9 => {
                let item_type = self.ty()?;
                let len = self.int()?;
                if len < 0 || len > 1_000_000 {
                    return Err(format!("NBT 集合长度异常: {len}"));
                }
                let _ = self.collection_length();
                let mut values = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    values.push(self.payload(item_type, depth + 1)?);
                }
                NbtValue::List(NbtList { item_type, values })
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let t = self.ty()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let value = self.payload(t, depth + 1)?;
                    entries.push((name, value));
                }
                NbtValue::Compound(entries)
            }
            11 => {
                let len = self.int()?;
                if len < 0 || len > 1_000_000 {
                    return Err(format!("NBT 集合长度异常: {len}"));
                }
                let mut out = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    out.push(self.int()?);
                }
                NbtValue::IntArray(out)
            }
            12 => {
                let len = self.int()?;
                if len < 0 || len > 1_000_000 {
                    return Err(format!("NBT 集合长度异常: {len}"));
                }
                let mut out = Vec::with_capacity(len as usize);
                for _ in 0..len {
                    out.push(self.long()?);
                }
                NbtValue::LongArray(out)
            }
            other => return Err(format!("不支持的 NBT 类型: {other}")),
        })
    }
}

fn decompress_nbt(buf: &[u8]) -> Result<Vec<u8>, String> {
    if buf.len() >= 2 && buf[0] == 0x1f && buf[1] == 0x8b {
        let mut out = Vec::new();
        GzDecoder::new(buf)
            .take(MAX_DECOMPRESSED_NBT as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| e.to_string())?;
        if out.len() > MAX_DECOMPRESSED_NBT {
            return Err("NBT 文件过大".into());
        }
        return Ok(out);
    }
    if buf.len() >= 2 && buf[0] == 0x78 {
        // 0x78 也可能恰好是未压缩数据的一部分，失败按原数据继续解析
        let mut out = Vec::new();
        if ZlibDecoder::new(buf)
            .take(MAX_DECOMPRESSED_NBT as u64 + 1)
            .read_to_end(&mut out)
            .is_ok()
            && out.len() <= MAX_DECOMPRESSED_NBT
        {
            return Ok(out);
        }
    }
    if buf.len() > MAX_DECOMPRESSED_NBT {
        return Err("NBT 文件过大".into());
    }
    Ok(buf.to_vec())
}

/// 解析未压缩、gzip 或 zlib NBT 为根 Compound
pub fn parse_nbt(buf: &[u8]) -> Result<Vec<(String, NbtValue)>, String> {
    let raw = decompress_nbt(buf)?;
    let mut r = Reader { buf: &raw, off: 0 };
    let root_type = r.ty()?;
    if root_type != 10 {
        return Err("NBT 根节点不是 Compound".into());
    }
    r.string()?; // root name（通常为空）
    match r.payload(10, 0)? {
        NbtValue::Compound(entries) => Ok(entries),
        _ => Err("NBT 根节点不是 Compound".into()),
    }
}

/// 便捷取值
pub fn get<'a>(root: &'a [(String, NbtValue)], key: &str) -> Option<&'a NbtValue> {
    root.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub fn as_compound(v: &NbtValue) -> Option<&Vec<(String, NbtValue)>> {
    match v {
        NbtValue::Compound(c) => Some(c),
        _ => None,
    }
}

pub fn as_str(v: &NbtValue) -> Option<&str> {
    match v {
        NbtValue::String(s) => Some(s),
        _ => None,
    }
}

pub fn as_int(v: &NbtValue) -> Option<i64> {
    match v {
        NbtValue::Byte(b) => Some(*b as i64),
        NbtValue::Short(s) => Some(*s as i64),
        NbtValue::Int(i) => Some(*i as i64),
        NbtValue::Long(l) => Some(*l),
        _ => None,
    }
}

// ---------------- 写 ----------------

fn write_payload(out: &mut Vec<u8>, ty: u8, value: &NbtValue) -> Result<(), String> {
    match (ty, value) {
        (1, NbtValue::Byte(v)) => out.push(*v as u8),
        (2, NbtValue::Short(v)) => out.extend_from_slice(&v.to_be_bytes()),
        (3, NbtValue::Int(v)) => out.extend_from_slice(&v.to_be_bytes()),
        (4, NbtValue::Long(v)) => out.extend_from_slice(&v.to_be_bytes()),
        (8, NbtValue::String(v)) => {
            let bytes = v.as_bytes();
            out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
            out.extend_from_slice(bytes);
        }
        (9, NbtValue::List(list)) => {
            out.push(list.item_type);
            out.extend_from_slice(&(list.values.len() as i32).to_be_bytes());
            for item in &list.values {
                write_payload(out, list.item_type, item)?;
            }
        }
        (10, NbtValue::Compound(entries)) => {
            for (k, v) in entries {
                out.push(v.tag_type());
                let kb = k.as_bytes();
                out.extend_from_slice(&(kb.len() as u16).to_be_bytes());
                out.extend_from_slice(kb);
                write_payload(out, v.tag_type(), v)?;
            }
            out.push(0);
        }
        _ => return Err("不支持的写入类型".into()),
    }
    Ok(())
}

/// 序列化根 Compound 为 NBT 二进制（无压缩，gzip 由调用方决定）
pub fn write_nbt(root: Vec<(String, NbtValue)>) -> Result<Vec<u8>, String> {
    let mut out = vec![10u8];
    out.extend_from_slice(&0u16.to_be_bytes()); // root name ""
    write_payload(&mut out, 10, &NbtValue::Compound(root))?;
    Ok(out)
}

/// 构造 servers.dat 内容：servers: List[Compound{name, ip}]
pub fn build_servers_dat(list: &[(String, String)]) -> Vec<u8> {
    let mut out = vec![10u8];
    out.extend_from_slice(&0u16.to_be_bytes());
    out.push(9); // list
    out.extend_from_slice(b"servers");
    out.push(10); // item type: compound
    out.extend_from_slice(&(list.len() as i32).to_be_bytes());
    for (name, ip) in list {
        out.push(8);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(b"name");
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(8);
        out.extend_from_slice(&2u16.to_be_bytes());
        out.extend_from_slice(b"ip");
        out.extend_from_slice(&(ip.len() as u16).to_be_bytes());
        out.extend_from_slice(ip.as_bytes());
        out.push(0);
    }
    out.push(0);
    out
}

/// 解析结果转 JSON（诊断/展示用）
pub fn to_json(v: &NbtValue) -> Value {
    match v {
        NbtValue::Byte(b) => json!(b),
        NbtValue::Short(s) => json!(s),
        NbtValue::Int(i) => json!(i),
        NbtValue::Long(l) => json!(l),
        NbtValue::Float(f) => json!(f),
        NbtValue::Double(d) => json!(d),
        NbtValue::ByteArray(b) => json!(b),
        NbtValue::String(s) => json!(s),
        NbtValue::List(l) => Value::Array(l.values.iter().map(to_json).collect()),
        NbtValue::Compound(c) => {
            let mut m = serde_json::Map::new();
            for (k, v) in c {
                m.insert(k.clone(), to_json(v));
            }
            Value::Object(m)
        }
        NbtValue::IntArray(a) => json!(a),
        NbtValue::LongArray(a) => json!(a),
    }
}
