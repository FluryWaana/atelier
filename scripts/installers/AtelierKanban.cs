using System;
using System.Diagnostics;
using System.IO;
using System.Windows.Forms;

class AtelierKanban {
    [STAThread]
    static int Main(string[] args) {
        try {
            var root = AppDomain.CurrentDomain.BaseDirectory;
            var mode = args.Length == 0 ? "" : args[0];
            if (args.Length > 1 || (mode != "" && mode != "--stop" && mode != "--import-data" && mode != "--no-browser"))
                throw new Exception("Argument de lancement inconnu.");
            var start = new ProcessStartInfo(Path.Combine(root, "runtime", "node.exe"), "\"" + Path.Combine(root, "launcher.mjs") + "\" " + mode) {
                WorkingDirectory = root, UseShellExecute = false, CreateNoWindow = true, RedirectStandardError = true
            };
            using (var process = Process.Start(start)) {
                var error = process.StandardError.ReadToEnd();
                process.WaitForExit();
                if (process.ExitCode != 0) throw new Exception(error);
                return 0;
            }
        } catch (Exception error) {
            MessageBox.Show(error.Message, "Atelier Kanban", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }
}
