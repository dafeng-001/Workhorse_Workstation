import base64
code = "WM1.eyJtIjoiYmY3N2ZiZjc2MWM2NzMwZSIsImV4cCI6IjIwMjctMTItMzEiLCJ0aWVyIjoicHJvIn0=.5c6f6af9cc8f85162a6d937c2d28c136"
p = code.strip().split(".")
print("parts", len(p), p[0])
raw = base64.b64decode(p[1])
payload = raw.decode()
print("payload", payload)
SECRET = b"wm-workhorse-license-v1-2026"
def fnv1a(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h
def hmac_like(msg):
    return f"{fnv1a(SECRET+msg):016x}{fnv1a(msg+SECRET):016x}"
sig = hmac_like(payload.encode())
print("expect", sig)
print("actual", p[2])
print("match", sig == p[2])
