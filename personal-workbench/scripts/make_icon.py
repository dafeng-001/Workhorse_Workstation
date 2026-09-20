from PIL import Image, ImageDraw
import os

root = r"C:\Users\10259\XiaomiMiMoProjects\.mimo-sessions\2026-09-17\突然想搞一个个人工作台，每天告诉我大概有多少代码没提交，完成了多少行代码的编写，\personal-workbench"
icon_dir = os.path.join(root, "src-tauri", "icons")
os.makedirs(icon_dir, exist_ok=True)


def make(size: int, path: str) -> str:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = size * 0.12
    d.rounded_rectangle(
        [m, m, size - m, size - m], radius=size * 0.18, fill=(15, 20, 25, 255)
    )
    bar_w = size * 0.12
    d.rounded_rectangle(
        [size * 0.22, size * 0.28, size * 0.22 + bar_w, size * 0.72],
        radius=bar_w / 2,
        fill=(61, 156, 240, 255),
    )
    y0 = size * 0.34
    for i, wfrac in enumerate([0.34, 0.28, 0.38]):
        y = y0 + i * size * 0.12
        d.rounded_rectangle(
            [size * 0.42, y, size * 0.42 + size * wfrac, y + size * 0.06],
            radius=size * 0.02,
            fill=(232, 238, 244, 230),
        )
    img.save(path)
    return path


png = make(256, os.path.join(icon_dir, "icon.png"))
print("wrote", png, Image.open(png).size)
make(32, os.path.join(icon_dir, "32x32.png"))
make(128, os.path.join(icon_dir, "128x128.png"))

# Windows .ico for NSIS / exe resources
ico_path = os.path.join(icon_dir, "icon.ico")
Image.open(png).save(ico_path, format="ICO", sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("wrote", ico_path)
