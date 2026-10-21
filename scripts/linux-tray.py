import os
import subprocess
import sys
import threading
from pathlib import Path

try:
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('AyatanaAppIndicator3', '0.1')
    from gi.repository import AyatanaAppIndicator3 as AppIndicator, GLib, Gtk
except (ImportError, ValueError):
    print('Icône Atelier indisponible : installer les bibliothèques Python GTK 3 et Ayatana AppIndicator.', file=sys.stderr)
    sys.exit(1)


class Tray:
    def __init__(self, root):
        self.root = root
        self.busy = False
        self.closed = False
        self.menu = Gtk.Menu()
        for title, mode in [('Ouvrir Atelier', ''), ('Quitter', '--stop')]:
            item = Gtk.MenuItem(label=title)
            item.connect('activate', lambda _, mode=mode: self.launch(mode))
            self.menu.append(item)
        self.menu.show_all()
        self.indicator = AppIndicator.Indicator.new(
            'atelier-kanban-' + str(os.getpid()), str(root / 'public' / 'atelier-kanban.png'),
            AppIndicator.IndicatorCategory.APPLICATION_STATUS)
        self.indicator.set_title('Atelier Kanban')
        self.indicator.set_menu(self.menu)
        self.indicator.set_status(AppIndicator.IndicatorStatus.ACTIVE)
        # EOF removes the icon on shutdown, update, or an unexpected server exit.
        GLib.io_add_watch(sys.stdin.fileno(), GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR, self.disconnected)

    def disconnected(self, source, condition):
        if condition & (GLib.IO_HUP | GLib.IO_ERR) or not os.read(source, 4096):
            self.closed = True
            self.indicator.set_status(AppIndicator.IndicatorStatus.PASSIVE)
            Gtk.main_quit()
            return False
        return True

    def launch(self, mode):
        if self.busy or self.closed:
            return
        self.busy = True
        self.menu.set_sensitive(False)

        def run():
            try:
                command = [str(self.root / 'runtime' / 'node'), str(self.root / 'launcher.mjs')]
                result = subprocess.run(command + ([mode] if mode else []), cwd=self.root,
                                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.PIPE, text=True)
                error = result.stderr if result.returncode else ''
                if result.returncode and not error:
                    error = 'Le lanceur Atelier a échoué.'
            except OSError as exception:
                error = str(exception)
            GLib.idle_add(self.finished, error)

        threading.Thread(target=run, daemon=True).start()

    def finished(self, error):
        if self.closed:
            return False
        self.busy = False
        self.menu.set_sensitive(True)
        if error:
            dialog = Gtk.MessageDialog(message_type=Gtk.MessageType.ERROR, buttons=Gtk.ButtonsType.CLOSE,
                                       text='Atelier Kanban')
            dialog.format_secondary_text(error)
            dialog.connect('response', lambda dialog, _: dialog.destroy())
            dialog.show_all()
        return False


if __name__ == '__main__':
    try:
        if Gtk.init_check()[0]:
            tray = Tray(Path(__file__).resolve().parent.parent)
            Gtk.main()
    except Exception as error:
        print('Icône Atelier : ' + str(error), file=sys.stderr)
        sys.exit(1)
