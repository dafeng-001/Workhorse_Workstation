using System;
using System.IO;
using System.Text;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Shapes;

public class WpfLicenseTool : Window {
    TextBox machine, exp, tier, code, keyOut, licOut, packOut;
    static Brush Bg = (Brush)new BrushConverter().ConvertFrom("#0C1220");
    static Brush Card = (Brush)new BrushConverter().ConvertFrom("#16203A");
    static Brush Ink = (Brush)new BrushConverter().ConvertFrom("#E8EEF7");
    static Brush Dim = (Brush)new BrushConverter().ConvertFrom("#8A9BB0");
    static Brush Accent = (Brush)new BrushConverter().ConvertFrom("#4BA3FF");
    static Brush FieldBg = (Brush)new BrushConverter().ConvertFrom("#0A101C");
    static Brush BdBrush = (Brush)new BrushConverter().ConvertFrom("#2A3648");

    public WpfLicenseTool() {
        Title = "NiuMa License Tool";
        Width = 900;
        Height = 800;
        Background = Bg;
        FontFamily = new FontFamily("Microsoft YaHei UI");
        FontSize = 13;
        WindowStartupLocation = WindowStartupLocation.CenterScreen;
        ResizeMode = ResizeMode.CanMinimize;

        StackPanel root = new StackPanel();
        root.Margin = new Thickness(28, 20, 28, 24);

        root.Children.Add(MakeText("授权 / 更新包工具", 22, Ink, FontWeights.Bold));
        root.Children.Add(MakeText("生成激活码 · 密钥 · update.wmp（离线，与客户端同算法）", 12, Dim));
        root.Children.Add(MakeSpace(12));

        root.Children.Add(MakeSection("1 · 机器码"));
        machine = MakeInput("Customer machine id from app Settings (empty = any machine)");
        root.Children.Add(machine);
        // auto-detect local machine id
        {
            string mid = LocalMachineId();
            machine.Text = mid;
            machine.Tag = null;
            machine.Foreground = Ink;
        }
        root.Children.Add(MakeSpace(12));

        root.Children.Add(MakeSection("2 · 激活码"));
        exp = MakeInput("Expire yyyy-MM-dd (empty = never)");
        root.Children.Add(exp);
        root.Children.Add(MakeSpace(6));
        tier = MakeInput("tier (default pro)");
        tier.Text = "pro";
        tier.Tag = "tier (default pro)";
        root.Children.Add(tier);
        root.Children.Add(MakeSpace(8));
        StackPanel row1 = new StackPanel();
        row1.Orientation = Orientation.Horizontal;
        row1.Children.Add(MakeButton("Generate License", true, new RoutedEventHandler(OnLicense)));
        row1.Children.Add(MakeButton("Copy", false, new RoutedEventHandler(OnCopy)));
        root.Children.Add(row1);
        root.Children.Add(MakeSpace(6));
        licOut = MakeOutput();
        root.Children.Add(licOut);
        root.Children.Add(MakeSpace(12));

        root.Children.Add(MakeSection("3 · Update Key"));
        code = MakeInput("WM1.... license code");
        root.Children.Add(code);
        root.Children.Add(MakeSpace(6));
        root.Children.Add(MakeButton("Export Key Hex", true, new RoutedEventHandler(OnKey)));
        root.Children.Add(MakeSpace(6));
        keyOut = MakeOutput();
        keyOut.Height = 34;
        root.Children.Add(keyOut);
        root.Children.Add(MakeSpace(12));

        root.Children.Add(MakeSection("4 · Pack update.wmp"));
        root.Children.Add(MakeButton("Choose exe and pack...", true, new RoutedEventHandler(OnPack)));
        root.Children.Add(MakeSpace(6));
        packOut = MakeOutput();
        packOut.Height = 52;
        root.Children.Add(packOut);

        ScrollViewer sv = new ScrollViewer();
        sv.Content = root;
        sv.Background = Bg;
        sv.VerticalScrollBarVisibility = ScrollBarVisibility.Auto;
        Content = sv;
    }

    UIElement MakeSpace(double h) {
        return new Border();
    }

    TextBlock MakeText(string s, double size, Brush color) {
        return MakeText(s, size, color, FontWeights.Normal);
    }

    TextBlock MakeText(string s, double size, Brush color, FontWeight weight) {
        TextBlock tb = new TextBlock();
        tb.Text = s;
        tb.FontSize = size;
        tb.Foreground = color;
        tb.FontWeight = weight;
        tb.Margin = new Thickness(0, 2, 0, 6);
        return tb;
    }

    Border MakeSection(string title) {
        Border card = new Border();
        card.Background = Card;
        card.CornerRadius = new CornerRadius(12);
        card.Padding = new Thickness(16, 12, 16, 12);
        card.Margin = new Thickness(0, 4, 0, 4);
        StackPanel sp = new StackPanel();
        sp.Orientation = Orientation.Horizontal;
        Rectangle r = new Rectangle();
        r.Width = 4;
        r.Height = 20;
        r.Fill = Accent;
        r.RadiusX = 2;
        r.RadiusY = 2;
        sp.Children.Add(r);
        TextBlock tb = new TextBlock();
        tb.Text = title;
        tb.Foreground = Accent;
        tb.FontSize = 13;
        tb.FontWeight = FontWeights.Bold;
        tb.Margin = new Thickness(10, 0, 0, 0);
        tb.VerticalAlignment = VerticalAlignment.Center;
        sp.Children.Add(tb);
        card.Child = sp;
        return card;
    }

    TextBox MakeInput(string hint) {
        TextBox t = new TextBox();
        t.Margin = new Thickness(0, 4, 0, 4);
        t.Padding = new Thickness(10, 8, 10, 8);
        t.Background = FieldBg;
        t.Foreground = Dim;
        t.BorderBrush = BdBrush;
        t.BorderThickness = new Thickness(1);
        t.FontFamily = new FontFamily("Consolas");
        t.Height = 36;
        t.Text = hint;
        t.Tag = hint;
        t.GotFocus += delegate {
            if (object.ReferenceEquals(t.Tag, hint) || (t.Tag as string) == hint) {
                if (t.Text == hint) { t.Text = ""; t.Foreground = Ink; }
            }
        };
        t.LostFocus += delegate {
            if (t.Text.Trim() == "") {
                t.Text = hint;
                t.Tag = hint;
                t.Foreground = Dim;
            } else {
                t.Foreground = Ink;
            }
        };
        return t;
    }

    TextBox MakeOutput() {
        TextBox t = new TextBox();
        t.Margin = new Thickness(0, 6, 0, 6);
        t.Padding = new Thickness(10, 8, 10, 8);
        t.MinHeight = 52;
        t.AcceptsReturn = true;
        t.TextWrapping = TextWrapping.Wrap;
        t.Background = (Brush)new BrushConverter().ConvertFrom("#080C16");
        t.Foreground = (Brush)new BrushConverter().ConvertFrom("#A0DCB4");
        t.BorderBrush = BdBrush;
        t.BorderThickness = new Thickness(1);
        t.FontFamily = new FontFamily("Consolas");
        t.FontSize = 12;
        return t;
    }

    Button MakeButton(string label, bool primary, RoutedEventHandler h) {
        Button b = new Button();
        b.Content = label;
        b.Margin = new Thickness(0, 4, 10, 4);
        b.Padding = new Thickness(18, 10, 18, 10);
        b.BorderThickness = new Thickness(0);
        b.FontSize = 13;
        b.FontWeight = FontWeights.SemiBold;
        if (primary) {
            b.Background = Accent;
            b.Foreground = (Brush)new BrushConverter().ConvertFrom("#061018");
        } else {
            b.Background = (Brush)new BrushConverter().ConvertFrom("#202A3A");
            b.Foreground = Ink;
        }
        b.Click += h;
        return b;
    }

    static ulong Fnv(byte[] data) {
        ulong h = 0xcbf29ce484222325UL;
        for (int i = 0; i < data.Length; i++) {
            h ^= (ulong)data[i];
            h *= 0x100000001b3UL;
        }
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
        byte[] sc = Encoding.UTF8.GetBytes(code == null ? "" : code);
        byte[] sm = Encoding.UTF8.GetBytes(machine == null ? "" : machine);
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

    string Val(TextBox t) {
        string hint = t.Tag as string;
        if (hint != null && t.Text == hint) return "";
        return (t.Text == null ? "" : t.Text).Trim();
    }

    
    static string LocalMachineId() {
        string host = Environment.GetEnvironmentVariable("COMPUTERNAME");
        if (host == null || host.Length == 0) host = Environment.GetEnvironmentVariable("HOSTNAME");
        if (host == null) host = "";
        string user = Environment.GetEnvironmentVariable("USERNAME");
        if (user == null || user.Length == 0) user = Environment.UserName;
        if (user == null) user = "";
        string raw = host.ToLower().Trim() + "|" + user.ToLower().Trim() + "|wm";
        return Fnv(Encoding.UTF8.GetBytes(raw)).ToString("x16");
    }

    void OnLicense(object s, RoutedEventArgs e) {
        string m = Val(machine);
        string ex = Val(exp);
        string tr = Val(tier);
        if (tr == "") tr = "pro";
        string payload = "{\"m\":\"" + m + "\",\"exp\":\"" + ex + "\",\"tier\":\"" + tr + "\"}";
        byte[] pb = Encoding.UTF8.GetBytes(payload);
        licOut.Text = "WM1." + Convert.ToBase64String(pb) + "." + Hmac(pb);
    }

    void OnCopy(object s, RoutedEventArgs e) {
        if (licOut.Text != null && licOut.Text.Length > 0) {
            Clipboard.SetText(licOut.Text);
            MessageBox.Show("Copied license code", "OK");
        }
    }

    void OnKey(object s, RoutedEventArgs e) {
        string c = Val(code);
        if (!c.StartsWith("WM1.")) {
            MessageBox.Show("Please input WM1 license first", "Tip");
            return;
        }
        byte[] k = UpdateKey(c, Val(machine));
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < k.Length; i++) sb.Append(k[i].ToString("x2"));
        keyOut.Text = sb.ToString();
    }

    void OnPack(object s, RoutedEventArgs e) {
        string c = Val(code);
        if (!c.StartsWith("WM1.")) {
            MessageBox.Show("Please generate WM1 license first", "Tip");
            return;
        }
        Microsoft.Win32.OpenFileDialog ofd = new Microsoft.Win32.OpenFileDialog();
        ofd.Filter = "Exe|*.exe|All|*.*";
        if (ofd.ShowDialog() != true) return;
        Microsoft.Win32.SaveFileDialog sfd = new Microsoft.Win32.SaveFileDialog();
        sfd.FileName = "update.wmp";
        sfd.Filter = "WMP|*.wmp";
        if (sfd.ShowDialog() != true) return;
        byte[] plain = File.ReadAllBytes(ofd.FileName);
        byte[] key = UpdateKey(c, Val(machine));
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
        packOut.Text = "Saved: " + sfd.FileName;
    }

    [STAThread]
    public static void Main() {
        Application app = new Application();
        app.Run(new WpfLicenseTool());
    }
}
