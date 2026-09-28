#!/usr/bin/env python3
"""WM license / update-pack tool (no server)."""
import argparse, base64, struct, os, getpass
from pathlib import Path

SECRET = b"wm-workhorse-license-v1-2026"

def fnv1a(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h

def hmac_like(msg: bytes) -> str:
    return f"{fnv1a(SECRET+msg):016x}{fnv1a(msg+SECRET):016x}"

def machine_id() -> str:
    host = os.environ.get("COMPUTERNAME", os.environ.get("HOSTNAME", "")).lower().strip()
    user = os.environ.get("USERNAME", getpass.getuser()).lower().strip()
    return f"{fnv1a(f'{host}|{user}|wm'.encode()):016x}"

def update_key(lic_code: str, machine: str) -> bytes:
    seed = SECRET + lic_code.encode() + machine.encode()
    key = bytearray(32)
    for i in range(4):
        h = hmac_like(seed + bytes([i])).encode()
        for j in range(8):
            x = h[j] if j < len(h) else 48
            y = h[j+8] if j+8 < len(h) else 0
            key[i*8+j] = x ^ y
    return bytes(key)

def keystream_xor(key: bytes, data: bytearray):
    for i in range(len(data)):
        k = key[i % len(key)] ^ ((i // len(key)) & 0xFF)
        k = (k * 31) & 0xFF if False else (k ^ ((i // len(key) or 0) * 31 & 0xFF))
        data[i] ^= k & 0xFF

def keystream_xor2(key: bytes, data: bytearray):
    for i in range(len(data)):
        k = key[i % len(key)] ^ (((i // len(key)) * 31) & 0xFF)
        data[i] ^= k

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["machine", "license", "key", "pack"])
    ap.add_argument("--machine", default="")
    ap.add_argument("--exp", default="")
    ap.add_argument("--tier", default="pro")
    ap.add_argument("--code", default="")
    ap.add_argument("--exe", default="")
    ap.add_argument("--out", default="")
    a = ap.parse_args()
    if a.cmd == "machine":
        print(machine_id())
    elif a.cmd == "license":
        m = a.machine
        payload = f'{{"m":"{m}","exp":"{a.exp}","tier":"{a.tier}"}}'
        pb = payload.encode()
        print("WM1." + base64.b64encode(pb).decode() + "." + hmac_like(pb))
    elif a.cmd == "key":
        if not a.code:
            raise SystemExit("need --code")
        m = a.machine or machine_id()
        print(update_key(a.code, m).hex())
    elif a.cmd == "pack":
        if not a.code:
            raise SystemExit("need --code")
        m = a.machine or machine_id()
        exe = a.exe or str(Path(__file__).resolve().parent.parent / "personal-workbench/src-tauri/target/release/personal-workbench.exe")
        out = a.out or str(Path(__file__).resolve().parent.parent / "update.wmp")
        key = update_key(a.code, m)
        plain = bytearray(Path(exe).read_bytes())
        keystream_xor2(key, plain)
        mac = hmac_like(key + bytes(plain))[:16]
        blob = b"WMP1" + bytes(plain) + mac.encode()
        Path(out).write_bytes(blob)
        print(f"pack: {out} ({len(blob)} bytes)")
        print(f"machine: {m}")

if __name__ == "__main__":
    main()
