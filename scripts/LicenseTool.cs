using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Text;
using System.Windows.Forms;

public class LicenseTool : Form {
    TextBox machine, exp, tier, code, keyOut, licOut, packOut;
    Color Bg = Color.FromArgb(12, 18, 32);
    Color Card = Color.FromArgb(22, 30, 46);
    Color Line = Color.FromArgb(45, 58, 78);
    Color Ink = Color.FromArgb(232, 238, 247);
    Color Dim = Color.FromArgb(138, 155, 176);
    Color Accent = Color.FromArgb(74, 163, 255);

    public LicenseTool() {
        Text = "牛马工作台 · 授权工具";
        ClientSize = new Size(880, 780);
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Bg;
        Font = new Font("Microsoft YaHei UI", 9.5f);
        FormBorderStyle = FormBorderStyle.FixedDialog;
        MaximizeBox = false;
        MinimizeBox = true;

        var title = new Label {
            Text = "授权 / 更新包工具",
            Font = new Font("Microsoft YaHei UI", 16f, FontStyle.Bold),
            ForeColor = Ink, Left = 28, Top = 22, Width = 500, Height = 32
        };
        Controls.Add(title);
        var sub = new Label {
            Text = "生成激活码 · 导出密钥 · 打包 update.wmp（与客户端同一算法，本地离线）",
            ForeColor = Dim, Left = 28, Top = 56, Width = 800, Height = 22
        };
        Controls.Add(sub);

        int y = 96;
        y = Section("1 · 机器码", y);
        machine = Field(y, "客户机器码（应用设置可复制；留空 = 不限机器）");
        y += 72;

        y = Section("2 · 激活码", y);
        exp = Field(y, "到期日（yyyy-MM-dd，留空 = 永久）");
        y += 72;
        tier = Field(y, "版本 tier（默认 pro）");
        tier.Text = "pro";
        y += 72;
        Btn("生成激活码", 28, y, 130, OnLicense, true);
        Btn("复制", 170, y, 90, OnCopyLic, false);
        y += 48;
        licOut = OutBox(y, 56);
        y += 72;

        y = Section("3 · 更新包密钥（调试）", y);
        code = Field(y, "激活码 WM1....");
        y += 72;
        Btn("导出密钥 Hex", 28, y, 150, OnKey, true);
        y += 48;
        keyOut = Field(y, "密钥");
        y += 72;

        y = Section("4 · 打包 update.wmp", y);
        Btn("选择 exe 并打包…", 28, y, 180, OnPack, true);
        y += 48;
        packOut = OutBox(y, 52);
    }

    int Section(string t, int y) {
        var p = new Panel {
            Left = 20, Top = y, Width = 840, Height = 40,
            BackColor = Card, BorderStyle = BorderStyle.None
        };
        var l = new Label {
            Text = t, Left = 14, Top = 10, Width = 800,
            ForeColor = Accent, Font = new Font("Microsoft YaHei UI", 10f, FontStyle.Bold)
        };
        p.Controls.Add(l);
        Controls.Add(p);
        // accent bar
        var bar = new Panel { Left = 20, Top = y, Width = 4, Height = 40, BackColor = Accent };
        Controls.Add(bar);
        return y + 48;
    }

    TextBox Field(int y, string placeholder) {
        var t = new TextBox {
            Left = 28, Top = y, Width = 824, Height = 32,
            BackColor = Color.FromArgb(10, 16, 28),
            ForeColor = Ink,
            BorderStyle = BorderStyle.FixedSingle,
            Font = new Font("Consolas", 10f)
        };
        t.GotFocus += (s, e) => { if (t.Text == placeholder) { t.Text = ""; t.ForeColor = Ink; } };
        t.LostFocus += (s, e) => { if (t.Text.Trim() == "") { t.Text = placeholder; t.ForeColor = Color.FromArgb(90, 105, 125); } };
        t.Text = placeholder;
        t.ForeColor = Color.FromArgb(90, 105, 125);
        Controls.Add(t);
        return t;
    }

    TextBox OutBox(int y, int h) {
        var t = new TextBox {
            Left = 28, Top = y, Width = 824, Height = h,
            Multiline = true, ScrollBars = ScrollBars.Vertical,
            BackColor = Color.FromArgb(8, 12, 22),
            ForeColor = Color.FromArgb(160, 220, 180),
            BorderStyle = BorderStyle.FixedSingle,
            Font = new Font("Consolas", 9.5f)
        };
        Controls.Add(t);
        return t;
    }

    void Btn(string text, int x, int y, int w, EventHandler h, bool primary) {
        var b = new Button {
            Text = text, Left = x, Top = y, Width = w, Height = 34,
            FlatStyle = FlatStyle.Flat,
            BackColor = primary ? Accent : Color.FromArgb(32, 42, 58),
            ForeColor = primary ? Color.FromArgb(6, 16, 24) : Ink,
            Font = new Font("Microsoft YaHei UI", 9.5f, FontStyle.Bold),
            Cursor = Cursors.Hand
        };
        b.FlatAppearance.BorderSize = 0;
        b.Click += h;
        Controls.Add(b);
    }

    static ulong Fnv(byte[] data) {
        ulong h = 0xcbf29ce484222325UL;
        for (int i = 0; i < data.Length; i++) { h ^= (ulong)data[i]; h *= 0x100000001b3UL; }
        return h;
    }
    static string Hmac(byte[] msg) {
        byte[] sec = Encoding.UTF8.GetBytes("wm-workhorse-license-v1-2026");
        byte[] a = new byte[sec.Length + msg.Length];
        Buffer.BlockCopy(sec, 0, a, 0, sec.Length);
        Buffer.BlockCopy(msg, 0, a, sec.Length, msg.Length);
        byte[] b = new byte[msg.Length + sec.Length];
        Buffer.BlockCopy(msg, 0, b, 0, msg.Length);
        Buffer.BlockCopy(sec, 0, b, msg.Length, sec.Length);
        return Fnv(a).ToString("x16") + Fnv(b).ToString("x16");
    }
    static byte[] UpdateKey(string code, string machine) {
        byte[] sec = Encoding.UTF8.GetBytes("wm-workhorse-license-v1-2026");
        byte[] sc = Encoding.UTF8.GetBytes(code ?? "");
        byte[] sm = Encoding.UTF8.GetBytes(machine ?? "");
        byte[] seed = new byte[sec.Length + sc.Length + sm.Length];
        Buffer.BlockCopy(sec, 0, seed, 0, sec.Length);
        Buffer.BlockCopy(sc, 0, seed, sec.Length, sc.Length);
        Buffer.BlockCopy(sm, 0, seed, sec.Length + sc.Length, sm.Length);
        byte[] key = new byte[32];
        for (int i = 0; i < 4; i++) {
            byte[] c = new byte[seed.Length + 1];
            Buffer.BlockCopy(seed, 0, c, 0, seed.Length);
            c[seed.Length] = (byte)i;
            byte[] h = Encoding.UTF8.GetBytes(Hmac(c));
            for (int j = 0; j < 8; j++) {
                int x = h[j];
                int yv = (j + 8 < h.Length) ? h[j + 8] : 0;
                key[i * 8 + j] = (byte)(x ^ yv);
            }
        }
        return key;
    }

    string RealText(TextBox t) {
        string s = t.Text.Trim();
        if (s.StartsWith("（") || s.ToLower().Contains("machine") || s.Contains("留空") || s.Contains("到期") || s.Contains("tier") || s.Contains("WM1") || s.Contains("密钥") || s == "pro" && t == tier)
            return t == tier ? "pro" : "";
        return s;
    }

    void OnLicense(object s, EventArgs e) {
        string m = machine.Text;
        if (m.Contains("机器") || m.Contains("留空") || m.Trim() == "") m = "";
        string ex = exp.Text;
        if (ex.Contains("到期") || ex.Contains("留空") || ex.Trim() == "") ex = "";
        string tr = tier.Text.Trim();
        if (tr == "" || tr.Contains("tier") || tr.Contains("版本")) tr = "pro";
        string payload = "{\"m\":\"" + m.Trim() + "\",\"exp\":\"" + ex.Trim() + "\",\"tier\":\"" + tr + "\"}";
        byte[] pb = Encoding.UTF8.GetBytes(payload);
        licOut.Text = "WM1." + Convert.ToBase64String(pb) + "." + Hmac(pb);
    }

    void OnCopyLic(object s, EventArgs e) {
        try { Clipboard.SetText(licOut.Text); MessageBox.Show("已复制激活码", "OK", MessageBoxButtons.OK, MessageBoxIcon.Information); } catch { }
    }

    void OnKey(object s, EventArgs e) {
        string c = code.Text.Trim();
        if (!c.StartsWith("WM1.")) {
            MessageBox.Show("请先粘贴或生成 WM1 激活码", "提示", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return;
        }
        string m = machine.Text;
        if (m.Contains("机器") || m.Contains("留空")) m = "";
        byte[] k = UpdateKey(c, m.Trim());
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < k.Length; i++) sb.Append(k[i].ToString("x2"));
        keyOut.Text = sb.ToString();
    }

    void OnPack(object s, EventArgs e) {
        string c = code.Text.Trim();
        if (!c.StartsWith("WM1.")) {
            MessageBox.Show("请先生成 WM1 激活码", "提示", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return;
        }
        OpenFileDialog ofd = new OpenFileDialog();
        ofd.Filter = "可执行文件|*.exe|全部|*.*";
        ofd.Title = "选择要打包的 exe";
        if (ofd.ShowDialog() != DialogResult.OK) return;
        SaveFileDialog sfd = new SaveFileDialog();
        sfd.FileName = "update.wmp";
        sfd.Filter = "更新包|*.wmp";
        sfd.Title = "保存加密更新包";
        if (sfd.ShowDialog() != DialogResult.OK) return;
        byte[] plain = File.ReadAllBytes(ofd.FileName);
        string m = machine.Text;
        if (m.Contains("机器") || m.Contains("留空")) m = "";
        byte[] key = UpdateKey(c, m.Trim());
        for (int i = 0; i < plain.Length; i++) {
            int kk = key[i % key.Length] ^ (((i / key.Length) * 31) & 0xff);
            plain[i] = (byte)(plain[i] ^ kk);
        }
        byte[] chk = new byte[key.Length + plain.Length];
        Buffer.BlockCopy(key, 0, chk, 0, key.Length);
        Buffer.BlockCopy(plain, 0, chk, key.Length, plain.Length);
        byte[] mac = Encoding.ASCII.GetBytes(Hmac(chk).Substring(0, 16));
        FileStream fs = File.Create(sfd.FileName);
        fs.Write(Encoding.ASCII.GetBytes("WMP1"), 0, 4);
        fs.Write(plain, 0, plain.Length);
        fs.Write(mac, 0, mac.Length);
        fs.Close();
        packOut.Text = "已保存: " + sfd.FileName + "\r\n机器码: " + (m.Trim() == "" ? "(不限)" : m.Trim());
    }

    [STAThread]
    public static void Main() {
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.Run(new LicenseTool());
    }
}
