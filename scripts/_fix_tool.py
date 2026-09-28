from pathlib import Path
p = Path(r"C:\Users\10259\XiaomiMiMoProjects\牛马工作台\scripts\license-tool.html")
t = p.read_text(encoding="utf-8")
t = t.replace("  const seed = new TextEncoder().encode(SECRET.toString() && \"\"); // placeholder\n", "")
old = '''$("btn-mac") && document.getElementById("btn-mac").addEventListener("click", () => {
  const el = document.getElementById("machine");
  if (!el.value.trim()) el.value = "（示例）bf77fbf761c6730e";
  document.getElementById("key-out").textContent = "ok";
});
function $(id) { return document.getElementById(id); }
'''
t = t.replace(old, "function $(id) { return document.getElementById(id); }\n")
p.write_text(t, encoding="utf-8")
print("fixed")
