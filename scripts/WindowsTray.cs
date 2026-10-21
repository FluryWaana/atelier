using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Threading.Tasks;
using System.Windows.Forms;

class WindowsTray : ApplicationContext {
    readonly string root = Path.GetFullPath(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, ".."));
    readonly NotifyIcon icon;
    readonly ContextMenuStrip menu = new ContextMenuStrip();
    readonly Timer timer = new Timer { Interval = 100 };
    bool busy;

    [STAThread]
    static int Main() {
        try {
            Application.EnableVisualStyles();
            using (var tray = new WindowsTray()) Application.Run(tray);
            return 0;
        } catch (Exception error) {
            Console.Error.WriteLine("Icône Atelier : " + error.Message);
            return 1;
        }
    }

    public WindowsTray() {
        menu.Items.Add("Ouvrir Atelier", null, async (sender, args) => await Launch(""));
        menu.Items.Add("Quitter", null, async (sender, args) => await Launch("--stop"));
        icon = new NotifyIcon { Text = "Atelier Kanban", Icon = new Icon(Path.Combine(root, "public", "atelier-kanban.ico")), ContextMenuStrip = menu, Visible = true };
        icon.DoubleClick += async (sender, args) => await Launch("");
        // The server owns stdin: EOF also removes the icon after a crash or an update.
        var disconnected = Task.Run(() => Console.In.ReadToEnd());
        timer.Tick += (sender, args) => { if (disconnected.IsCompleted) ExitThread(); };
        timer.Start();
    }

    async Task Launch(string mode) {
        if (busy) return;
        busy = true;
        menu.Enabled = false;
        try {
            await Task.Run(() => {
                var start = new ProcessStartInfo(Path.Combine(root, "runtime", "node.exe"), "\"" + Path.Combine(root, "launcher.mjs") + "\" " + mode) {
                    WorkingDirectory = root, UseShellExecute = false, CreateNoWindow = true, RedirectStandardError = true
                };
                using (var process = Process.Start(start)) {
                    var error = process.StandardError.ReadToEnd();
                    process.WaitForExit();
                    if (process.ExitCode != 0) throw new Exception(error);
                }
            });
        } catch (Exception error) {
            if (icon.Visible) MessageBox.Show(error.Message, "Atelier Kanban", MessageBoxButtons.OK, MessageBoxIcon.Error);
        } finally {
            busy = false;
            if (!menu.IsDisposed) menu.Enabled = true;
        }
    }

    protected override void Dispose(bool disposing) {
        if (disposing && !menu.IsDisposed) {
            timer.Dispose();
            icon.Visible = false;
            icon.Icon.Dispose();
            icon.Dispose();
            menu.Dispose();
        }
        base.Dispose(disposing);
    }
}
