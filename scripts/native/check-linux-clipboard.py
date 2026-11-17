#!/usr/bin/env python3
"""Verify actual Wayland/Xlib app clipboard effects with a native Wayland reader.

Requires KWin, Qt6Core development files, wayland-scanner and a C++ compiler. Every offered
owner MIME payload remains in RAM/private pipes. The original snapshot remains
alive until independent readback after app/provider cleanup. A failed restore
retains that RAM snapshot and stops further test writes for recovery.
Shared actions are driven through AppHandle; physical bindings/menu routing are
covered by Rune runtime tests. Windows/macOS native execution remains pending.
"""
import argparse
import ctypes
import importlib.util
import json
import os
from pathlib import Path
import select
import subprocess
import tempfile
import time


class Owner:
    def __init__(self, binary, env=None):
        self.process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
        assert self.process.stdout.readline().strip() == 'ready', 'native clipboard reader failed'

    def command(self, command):
        self.process.stdin.write(json.dumps(command) + '\n')
        self.process.stdin.flush()
        assert select.select([self.process.stdout], [], [], 10)[0], 'clipboard reader timed out'
        return json.loads(self.process.stdout.readline())

    def close(self):
        self.process.stdin.close()
        self.process.wait(10)


def x_selection_empty():
    x = ctypes.CDLL('libX11.so.6')
    x.XOpenDisplay.restype = ctypes.c_void_p
    x.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
    x.XInternAtom.restype = ctypes.c_ulong
    x.XGetSelectionOwner.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x.XGetSelectionOwner.restype = ctypes.c_ulong
    x.XCloseDisplay.argtypes = [ctypes.c_void_p]
    display = x.XOpenDisplay(None)
    assert display, 'native X11 display unavailable'
    empty = x.XGetSelectionOwner(display, x.XInternAtom(display, b'CLIPBOARD', 0)) == 0
    x.XCloseDisplay(display)
    return empty


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--backend', choices=['wayland', 'x11'], required=True)
    parser.add_argument('--report', required=True)
    parser.add_argument('--data-control-xml', required=True)
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
    x_reader = None
    with tempfile.TemporaryDirectory(prefix='rodtty-native-clipboard-') as directory:
        directory = Path(directory)
        helper = directory / 'clipboard-owner'
        xml = Path(args.data_control_xml)
        assert xml.is_file(), 'native data-control protocol XML unavailable'
        header, code, obj = directory / 'clipboard-data-control-client.h', directory / 'clipboard-data-control.c', directory / 'clipboard-data-control.o'
        subprocess.run(['wayland-scanner', 'client-header', str(xml), str(header)], check=True)
        subprocess.run(['wayland-scanner', 'private-code', str(xml), str(code)], check=True)
        subprocess.run(['cc', '-c', str(code), '-o', str(obj)], check=True)
        flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'Qt6Core', 'wayland-client'], text=True).split()
        subprocess.run(['c++', '-std=c++17', '-fPIC', '-pie', '-pthread', '-I', str(directory), str(Path(__file__).with_name('linux-clipboard-owner.cpp')), str(obj), '-o', str(helper), *flags], check=True)
        try:
            owner = Owner(helper)
            initial = owner.command({'snapshot': True})
            saved = initial.pop('content')  # Never include owner payloads in a report/log.
            assert initial['clipboard_access'] and saved, 'refusing test writes: native all-MIME snapshot unavailable'
            result['owner_mime_forms_snapshotted'] = len(saved)
            if args.backend == 'x11':
                assert x_selection_empty(), 'refusing Xlib test writes: existing X selection needs a full snapshot'
                result['x_selection_initially_empty'] = True
                x_helper = directory / 'xlib-clipboard'
                flags = subprocess.check_output(['pkg-config', '--cflags', '--libs', 'Qt6Widgets'], text=True).split()
                subprocess.run(['c++', '-std=c++17', '-fPIC', '-pie', str(Path(__file__).with_name('xlib-clipboard-probe.cpp')), '-o', str(x_helper), *flags], check=True)
                x_env = os.environ.copy()
                x_env['QT_QPA_PLATFORM'] = 'xcb'
                x_reader = Owner(x_helper, env=x_env)
            for suffix in ['', '.ready.json', '.copy.json', '.input.json']:
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
                compositor.pid = process.pid
                activated = compositor.query('if(target) workspace.activeWindow=target;')
                assert activated['target'], 'native clipboard app has no compositor window'
                time.sleep(.3)
                report.with_suffix('.copy.json').write_text('{"activated":true}')
                deadline = time.monotonic() + 15
                while True:
                    observed = (x_reader or owner).command({})
                    if observed['clipboard_access'] and observed['is_copy_fixture']:
                        break
                    assert time.monotonic() < deadline, 'native copy differs from exact Unicode fixture'
                    time.sleep(.025)
                assert (x_reader or owner).command({'set_fixture': True}).get('fixture_set'), 'clipboard fixture ownership changed'
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
            if x_reader:
                x_reader.close()
                result['x_selection_restored_empty'] = x_selection_empty()
                result['passed'] = result['passed'] and result['x_selection_restored_empty']
            if owner and saved:
                restored = owner.command({'restore': True, 'verify': saved}).get('original_forms_match', False)
                # Let the manager request/retain every restored form while this
                # native provider's event loop is still serving its source.
                time.sleep(1.5)
                owner.close()
                owner = None
                reader = Owner(helper)
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
