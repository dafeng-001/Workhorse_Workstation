//! 离线激活码：WM1.<b64 payload>.<hmac hex>
//! payload = { "m": machine_id, "exp": "YYYY-MM-DD"|"", "tier": "pro" }

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SECRET: &[u8] = b"wm-workhorse-license-v1-2026";
const PREFIX: &str = "WM1";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LicenseInfo {
    pub activated: bool,
    pub tier: String,
    pub exp: String,
    pub machine_id: String,
    pub message: String,
    #[serde(default)]
    pub code: String,
}

fn store_path() -> PathBuf {
    crate::config::app_root().join("data").join("license.json")
}

fn fnv1a(data: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn hmac_like(msg: &[u8]) -> String {
    // 双向混合校验（够个人软件离线验签用，非密码学 HSM）
    let mut buf = Vec::with_capacity(SECRET.len() + msg.len());
    buf.extend_from_slice(SECRET);
    buf.extend_from_slice(msg);
    let a = fnv1a(&buf);
    let mut buf2 = Vec::from(msg);
    buf2.extend_from_slice(SECRET);
    let b = fnv1a(&buf2);
    format!("{:016x}{:016x}", a, b)
}

pub fn machine_id() -> String {
    let host = std::env::var("COMPUTERNAME").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    let raw = format!("{}|{}|wm", host.trim().to_lowercase(), user.trim().to_lowercase());
    format!("{:016x}", fnv1a(raw.as_bytes()))
}

fn b64e(s: &str) -> String {
    use std::io::Write;
    // minimal base64
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let d = s.as_bytes();
    let mut o = String::new();
    for c in d.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        o.push(T[((n >> 18) & 63) as usize] as char);
        o.push(T[((n >> 12) & 63) as usize] as char);
        o.push(if c.len() > 1 { T[((n >> 6) & 63) as usize] as char } else { '=' });
        o.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    let _ = Write::flush(&mut std::io::sink());
    o
}

fn b64d(s: &str) -> Option<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for x in s.bytes() {
        if x == b'=' || x.is_ascii_whitespace() {
            continue;
        }
        let v = T.iter().position(|&t| t == x)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// 生成激活码（给售卖脚本用）：machine 可为空=任意机器
pub fn generate_code(machine: &str, exp: &str, tier: &str) -> String {
    let payload = format!("{{\"m\":\"{}\",\"exp\":\"{}\",\"tier\":\"{}\"}}", machine, exp, tier);
    let body = b64e(&payload);
    let sig = hmac_like(payload.as_bytes());
    format!("{PREFIX}.{body}.{sig}")
}

pub fn verify_code(code: &str) -> Result<(String, String, String), String> {
    let parts: Vec<&str> = code.trim().split('.').collect();
    if parts.len() != 3 || parts[0] != PREFIX {
        return Err("激活码格式不正确".into());
    }
    let raw = b64d(parts[1]).ok_or("激活码解码失败")?;
    let payload = String::from_utf8(raw).map_err(|_| "激活码内容无效")?;
    let sig = hmac_like(payload.as_bytes());
    if !sig.eq_ignore_ascii_case(parts[2]) {
        return Err("激活码校验失败（无效或被篡改）".into());
    }
    let v: serde_json::Value = serde_json::from_str(&payload).map_err(|_| "激活码内容无效")?;
    let m = v["m"].as_str().unwrap_or("").to_string();
    let exp = v["exp"].as_str().unwrap_or("").to_string();
    let tier = v["tier"].as_str().unwrap_or("pro").to_string();
    Ok((m, exp, tier))
}

pub fn load() -> LicenseInfo {
    let p = store_path();
    let mut info: LicenseInfo = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if info.activated {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(&info.exp, "%Y-%m-%d") {
            if !info.exp.is_empty() && d < chrono::Local::now().date_naive() {
                info.activated = false;
                info.message = "激活码已过期".into();
            }
        }
    }
    info.machine_id = machine_id();
    info
}

pub fn activate(code: &str) -> Result<LicenseInfo, String> {
    let (m, exp, tier) = verify_code(code)?;
    let me = machine_id();
    if !m.is_empty() && m != me {
        return Err(format!("该激活码绑定了其他机器（机器码 {}）", m));
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(&exp, "%Y-%m-%d") {
        if !exp.is_empty() && d < chrono::Local::now().date_naive() {
            return Err("激活码已过期".into());
        }
    }
    let info = LicenseInfo {
        activated: true,
        tier,
        exp,
        machine_id: me,
        message: "已激活 Pro".into(),
            code: code.trim().to_string(),
    };
    let p = store_path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&p, serde_json::to_string_pretty(&info).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    Ok(info)
}

pub fn clear() -> Result<LicenseInfo, String> {
    let p = store_path();
    let _ = std::fs::remove_file(p);
    Ok(LicenseInfo {
        activated: false,
        tier: String::new(),
        exp: String::new(),
        machine_id: machine_id(),
        message: "已取消激活".into(),
            code: String::new(),
    })
}

pub fn checksum16(data: &[u8]) -> String {
    hmac_like(data)[..16].to_string()
}

pub fn update_key() -> Option<Vec<u8>> {
    let lic = load();
    if !lic.activated || lic.code.is_empty() {
        return None;
    }
    let me = machine_id();
    if !lic.machine_id.is_empty() && lic.machine_id != me {
        return None;
    }
    let mut seed = Vec::from(SECRET);
    seed.extend_from_slice(lic.code.as_bytes());
    seed.extend_from_slice(me.as_bytes());
    let mut key = vec![0u8; 32];
    for i in 0..4 {
        let mut c = seed.clone();
        c.push(i as u8);
        let h = hmac_like(&c);
        let hb = h.as_bytes();
        for j in 0..8 {
            key[i * 8 + j] = hb.get(j).copied().unwrap_or(b'0') ^ hb.get(j + 8).copied().unwrap_or(0);
        }
    }
    Some(key)
}

pub fn is_pro() -> bool {
    load().activated
}
