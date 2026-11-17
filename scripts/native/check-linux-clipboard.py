#!/usr/bin/env python3
"""Verify actual Wayland/Xlib app clipboard effects with a native Wayland reader.

Requires KWin, Qt6Widgets development files and a C++ compiler. Every offered
owner MIME payload remains in RAM/private pipes. The original snapshot remains
alive until independent readback after app/provider cleanup. A failed restore
retains that RAM snapshot and stops further test writes for recovery.
Shared actions are driven through AppHandle; physical bindings/menu routing are
covered by Rune runtime tests. Windows/macOS native execution remains pending.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time


class Owner:
    def __init__(self, binary, compositor):
        self.compositor = compositor
        env = os.environ.copy()
        env['QT_QPA_PLATFORM'] = 'wayland'
        self.process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
        assert self.process.stdout.readline().strip() == 'ready', 'native clipboard reader failed'
        self.activate()

    def activate(self):
        self.compositor.pid = self.process.pid
        time.sleep(.25)
        self.compositor.query('if(target) workspace.activeWindow=target;')
        time.sleep(.4)

    def command(self, command):
        self.process.stdin.write(json.dumps(command) + '\n')
        self.process.stdin.flush()
        assert select.select([self.process.stdout], [], [], 10)[0], 'clipboard reader timed out'
        return json.loads(self.process.stdout.readline())

    def close(self):
        self.process.stdin.close()
        self.process.wait(10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--backend', choices=['wayland', 'x11'], required=True)
    parser.add_argument('--report', required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('frame_check', Path(__file__).with_name('check-linux-frame.py'))
    frame = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(frame)
    compositor = frame.KWin(0)
    previous = compositor.query()
    report = Path(args.report).resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    result = {'passed': False}
    saved = None
    owner = None
    process = None
    restored = False
    with tempfile.TemporaryDirectory(prefix='rodtty-native-clipboard-') as directory:
        directory = Path(directory)
        helper = directory / 'clipboard-owner'
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'Qt6Widgets'], text=True).split()
        subprocess.run(['c++', '-std=c++17', '-fPIC', '-pie', str(Path(__file__).with_name('linux-clipboard-owner.cpp')), '-o', str(helper), *flags], check=True)
        try:
            owner = Owner(helper, compositor)
            initial = owner.command({'snapshot': True})
            saved = initial.pop('content')  # Never include owner payloads in a report/log.
            assert initial['active'] and saved, 'refusing test writes: active all-MIME snapshot unavailable'
            result['owner_mime_forms_snapshotted'] = len(saved)
            for suffix in ['', '.ready.json', '.input.json']:
                (report if not suffix else report.with_suffix(suffix)).unlink(missing_ok=True)
            env = os.environ.copy()
            env['RODTTY_HOME'] = str(directory / 'owner')
            if args.backend == 'x11':
                env.pop('WAYLAND_DISPLAY', None)
                env.pop('WAYLAND_SOCKET', None)
            with open(str(report) + '.log', 'w') as log:
                process = subprocess.Popen([args.binary, '--clipboard-check', str(report), '--cwd', str(directory)], env=env, stdout=log, stderr=log)
                deadline = time.monotonic() + 30
                while not report.with_suffix('.ready.json').exists():
                    assert process.poll() is None and time.monotonic() < deadline, 'native app did not reach copy checkpoint'
                    time.sleep(.025)
                owner.activate()
                observed = owner.command({})
                assert observed['active'] and observed['is_fixture'], 'native copy differs from exact Unicode fixture'
                assert owner.command({'set_fixture': True}).get('fixture_set'), 'clipboard fixture ownership changed'
                report.with_suffix('.input.json').write_text('{"copy_verified":true}')
                assert process.wait(30) == 0, 'native app clipboard check failed'
            native = json.loads(report.read_text())
            expected = 'Wayland' if args.backend == 'wayland' else 'Xlib'
            assert native['passed'] and native['window_backend'] == expected, 'wrong backend or native failure'
            result.update(native)
        except Exception as error:
            result.update(passed=False, error=str(error))
        finally:
            if process and process.poll() is None:
                process.terminate()
                process.wait(10)
            if owner and saved:
                restored = owner.command({'restore': True, 'verify': saved}).get('original_forms_match', False)
                owner.close()
                owner = None
                reader = Owner(helper, compositor)
                independent = reader.command({'verify': saved})
                restored = restored and independent['original_forms_match']
                reader.close()
            if previous.get('priorFocus'):
                identifier = json.dumps(previous['priorFocus'])
                compositor.query(f'const priorWindow=workspace.windowList().find(w=>String(w.internalId)==={identifier}); if(priorWindow) workspace.activeWindow=priorWindow;')
            result['owner_clipboard_exact_after_provider_cleanup'] = restored
            result['passed'] = result['passed'] and restored
            report.with_suffix('.external.json').write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2), flush=True)
            if saved and not restored:
                # A recovery operator can inspect this live process; never discard the
                # sole backup or overwrite an unrelated concurrent clipboard update.
                print('Clipboard snapshot retained in RAM; further test writes stopped.', flush=True)
                while True:
                    time.sleep(30)
        assert result['passed'], 'native clipboard check failed'


if __name__ == '__main__':
    main()
