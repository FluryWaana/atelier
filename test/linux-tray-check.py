import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

source = Path(__file__).resolve().parent.parent / 'scripts' / 'linux-tray.py'
spec = importlib.util.spec_from_file_location('atelier_tray', source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
Gtk, GLib = module.Gtk, module.GLib
assert Gtk.init_check()[0], 'An Xvfb display is required.'

with tempfile.TemporaryDirectory(prefix='atelier-tray-') as directory:
    root = Path(directory)
    for name in ['scripts', 'runtime', 'public']:
        (root / name).mkdir()
    shutil.copyfile(source, root / 'scripts' / 'linux-tray.py')
    shutil.copyfile(source.parent.parent / 'public' / 'atelier-kanban.png', root / 'public' / 'atelier-kanban.png')
    (root / 'launcher.mjs').touch()
    (root / 'runtime' / 'node').write_text('''#!/usr/bin/python3
import os, sys
from pathlib import Path
root = Path(__file__).resolve().parent.parent
with (root / 'actions').open('a') as log:
    log.write((sys.argv[2] if len(sys.argv) > 2 else 'open') + ':' + os.environ['PORT'] + '\\n')
if (root / 'refuse').exists():
    print('Arrête les agents et chronomètres.', file=sys.stderr)
    sys.exit(1)
''')
    (root / 'runtime' / 'node').chmod(0o755)
    os.environ['PORT'] = '54321'
    reader, writer = os.pipe()
    os.dup2(reader, sys.stdin.fileno())
    os.close(reader)
    tray = module.Tray(root)
    step = 0
    failure = []
    deadline = time.monotonic() + 10

    def check():
        global step, writer
        try:
            assert time.monotonic() < deadline, 'Tray action timed out.'
            if tray.busy:
                return True
            actions = (root / 'actions').read_text().splitlines() if (root / 'actions').exists() else []
            if step == 0:
                tray.menu.get_children()[0].activate()
            elif step == 1:
                assert actions == ['open:54321']
                (root / 'refuse').touch()
                tray.menu.get_children()[1].activate()
            elif step == 2:
                assert actions == ['open:54321', '--stop:54321']
                dialogs = [window for window in Gtk.Window.list_toplevels() if isinstance(window, Gtk.MessageDialog)]
                assert len(dialogs) == 1
                assert 'Arrête les agents' in dialogs[0].get_property('secondary-text')
                assert not tray.closed and tray.menu.get_sensitive()
                dialogs[0].response(Gtk.ResponseType.CLOSE)
                (root / 'refuse').unlink()
                tray.menu.get_children()[1].activate()
            else:
                assert actions == ['open:54321', '--stop:54321', '--stop:54321']
                os.close(writer)
                writer = None
                return False
            step += 1
            return True
        except Exception as error:
            failure.append(error)
            Gtk.main_quit()
            return False

    GLib.timeout_add(50, check)
    Gtk.main()
    if writer is not None:
        os.close(writer)
    if failure:
        raise failure[0]
    assert tray.closed
    assert tray.indicator.get_status() == module.AppIndicator.IndicatorStatus.PASSIVE

    process = subprocess.Popen(['/usr/bin/python3', '-B', str(root / 'scripts' / 'linux-tray.py')], stdin=subprocess.PIPE)
    try:
        time.sleep(0.3)
        assert process.poll() is None
        process.stdin.close()
        assert process.wait(timeout=5) == 0
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
print('Linux tray: open, refused shutdown, retry and server disconnection passed.')
