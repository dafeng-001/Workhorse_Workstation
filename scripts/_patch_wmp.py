from pathlib import Path
root = Path(r"C:\Users\10259\XiaomiMiMoProjects\牛马工作台\personal-workbench\src-tauri\src")
p = root / "license.rs"
t = p.read_text(encoding="utf-8")
if "pub code:" not in t:
    t = t.replace("pub message: String,", "pub message: String,\n    #[serde(default)]\n    pub code: String,")
    t = t.replace('message: "已激活 Pro".into(),', 'message: "已激活 Pro".into(),\n            code: code.trim().to_string(),')
    t = t.replace('message: "已取消激活".into(),', 'message: "已取消激活".into(),\n            code: String::new(),')
if "fn checksum16" not in t:
    extra = '''pub fn checksum16(data: &[u8]) -> String {
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

pub fn is_pro()'''
    t = t.replace("pub fn is_pro()", extra)
p.write_text(t, encoding="utf-8")

u = root / "updater.rs"
s = u.read_text(encoding="utf-8")
if "fn decrypt_wmp" not in s:
    dec = '''const WMP_MAGIC: &[u8] = b"WMP1";

fn keystream_xor(key: &[u8], data: &mut [u8]) {
    for (i, b) in data.iter_mut().enumerate() {
        let k = key[i % key.len()] ^ ((i / key.len()) as u8).wrapping_mul(31);
        *b ^= k;
    }
}

pub fn decrypt_wmp(blob: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 4 + 16 + 4 || &blob[..4] != WMP_MAGIC {
        return Err("更新包格式不正确".into());
    }
    let body = &blob[4..blob.len() - 16];
    let mac = String::from_utf8_lossy(&blob[blob.len() - 16..]).to_string();
    let mut plain = body.to_vec();
    keystream_xor(key, &mut plain);
    let mut chk = Vec::from(key);
    chk.extend_from_slice(&plain);
    let expect = crate::license::checksum16(&chk);
    if !expect.eq_ignore_ascii_case(mac.trim()) {
        return Err("更新包校验失败（不适用于本机激活码/机器码）".into());
    }
    if plain.len() < 2 || plain[0] != b'M' || plain[1] != b'Z' {
        return Err("解密后不是合法 exe".into());
    }
    Ok(plain)
}

fn update_dir()'''
    s = s.replace("fn update_dir()", dec, 1)
    old = '''    if !(buf.len() >= 2 && buf[0] == b'M' && buf[1] == b'Z') {
        return Err("下载内容不是合法 Windows 程序（MZ）".into());
    }'''
    new = '''    if buf.len() > 4 && &buf[..4] == WMP_MAGIC {
        let key = crate::license::update_key().ok_or("更新包已加密：请先激活（密钥=激活码+机器码）")?;
        buf = decrypt_wmp(&buf, &key)?;
    }
    if !(buf.len() >= 2 && buf[0] == b'M' && buf[1] == b'Z') {
        return Err("下载内容不是合法 Windows 程序（MZ）".into());
    }'''
    if old in s:
        s = s.replace(old, new)
u.write_text(s, encoding="utf-8")
print("ok")
