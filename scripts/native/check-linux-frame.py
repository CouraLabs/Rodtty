#!/usr/bin/env python3
"""Real KWin Wayland/Xlib frame input + Rodtty lifecycle check.

Requires python dbus/GLib, KWin, cc, wayland-scanner and a KDE
plasma-wayland-protocols fake-input.xml for Wayland input. X11 uses libXtst.
Only the launched isolated process is queried or manipulated. The compositor
is the independent source for minimized/frame geometry; winit's unknown
Wayland minimized state is recorded as null, never promoted to an ack.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib


def wait_for(f, message, timeout=15):
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        result = f()
        if result:
            return result
        time.sleep(.04)
    raise AssertionError(message)


class KWin:
    def __init__(self, pid):
        DBusGMainLoop(set_as_default=True)
        self.bus = dbus.SessionBus()
        self.name = dbus.service.BusName('dev.rodtty.FrameCheck', self.bus)
        self.pid = pid
        self.payload = None
        owner = self

        class Reports(dbus.service.Object):
            @dbus.service.method('dev.rodtty.FrameCheck', in_signature='s', out_signature='')
            def report(self, payload):
                owner.payload = json.loads(payload)
                owner.loop.quit()

        self.receiver = Reports(self.bus, '/')
        self.counter = 0

    def query(self, action=''):
        self.counter += 1
        plugin = f'rodtty-frame-check-{self.pid}-{self.counter}'
        script = '''const target = workspace.windowList().find(w => Number(w.pid) === PID);
const prior = workspace.activeWindow;
ACTION
const point = workspace.cursorPos;
callDBus("dev.rodtty.FrameCheck", "/", "dev.rodtty.FrameCheck", "report", JSON.stringify({
priorFocus:prior ? String(prior.internalId) : null,
pointer:point ? [point.x,point.y] : null,
target:target ? {pid:target.pid,noBorder:target.noBorder,minimized:target.minimized,
frame:[target.frameGeometry.x,target.frameGeometry.y,target.frameGeometry.width,target.frameGeometry.height],
client:[target.clientGeometry.x,target.clientGeometry.y,target.clientGeometry.width,target.clientGeometry.height]} : null}));
'''.replace('PID', str(self.pid)).replace('ACTION', action)
        with tempfile.NamedTemporaryFile('w', suffix='.js') as source:
            source.write(script)
            source.flush()
            script_id = self.bus.call_blocking('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'loadScript', 'ss', (source.name, plugin))
            try:
                self.payload = None
                self.loop = GLib.MainLoop()
                self.bus.call_blocking('org.kde.KWin', f'/Scripting/Script{script_id}', 'org.kde.kwin.Script', 'run', '', ())
                timeout_id = GLib.timeout_add_seconds(5, lambda: self.loop.quit())
                self.loop.run()
                GLib.source_remove(timeout_id)
                assert self.payload is not None, 'KWin script did not report'
                return self.payload
            finally:
                self.bus.call_blocking('org.kde.KWin', '/Scripting', 'org.kde.kwin.Scripting', 'unloadScript', 's', (plugin,))


class Input:
    def __init__(self, args, temp):
        self.driver = None
        self.coordinate_scale, self.offset = 1.0, (0.0, 0.0)
        if args.backend == 'wayland' or args.fake_input_xml:
            assert args.fake_input_xml, '--fake-input-xml is required for Wayland'
            header, code = temp / 'fake-input-client.h', temp / 'fake-input.c'
            subprocess.run(['wayland-scanner', 'client-header', args.fake_input_xml, str(header)], check=True)
            subprocess.run(['wayland-scanner', 'private-code', args.fake_input_xml, str(code)], check=True)
            source = Path(__file__).with_name('wayland-frame-input.c')
            driver = temp / 'wayland-input'
            subprocess.run(['cc', '-Wall', '-Wextra', '-Werror', '-I', str(temp), str(source), str(code), '-lwayland-client', '-lm', '-o', str(driver)], check=True)
            self.driver = subprocess.Popen([str(driver)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        else:
            self.x = ctypes.CDLL('libX11.so.6')
            self.xt = ctypes.CDLL('libXtst.so.6')
            self.x.XOpenDisplay.restype = ctypes.c_void_p
            self.x.XFlush.argtypes = [ctypes.c_void_p]
            self.x.XCloseDisplay.argtypes = [ctypes.c_void_p]
            self.xt.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
            self.xt.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            self.xt.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
            self.display = self.x.XOpenDisplay(None)
            assert self.display, 'Xlib display unavailable'

    def send(self, op, x, y):
        if self.driver:
            self.driver.stdin.write(f'{op} {x} {y}\n')
            self.driver.stdin.flush()
            assert self.driver.stdout.readline().strip() == 'ok', 'Wayland input driver failed'
        else:
            if op == 'move':
                self.xt.XTestFakeMotionEvent(self.display, -1, round(x * self.coordinate_scale + self.offset[0]), round(y * self.coordinate_scale + self.offset[1]), 0)
            elif op == 'key':
                self.xt.XTestFakeKeyEvent(self.display, 9, int(y), 0)
            else:
                self.xt.XTestFakeButtonEvent(self.display, 1, int(y), 0)
            self.x.XFlush(self.display)
        time.sleep(.12)

    def move(self, x, y): self.send('move', x, y)
    def down(self): self.send('button', 272, 1)
    def up(self): self.send('button', 272, 0)
    def click(self, x, y):
        self.move(x, y)
        self.down()
        self.up()

    def close(self):
        self.up()
        if self.driver:
            self.driver.stdin.close()
            self.driver.wait(timeout=5)
        else:
            self.x.XCloseDisplay(self.display)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True)
    parser.add_argument('--backend', choices=['wayland', 'x11'], required=True)
    parser.add_argument('--report', required=True)
    parser.add_argument('--fake-input-xml')
    parser.add_argument('--screenshot')
    args = parser.parse_args()
    report = Path(args.report).resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    for suffix in ['.ready.json', '.input.json', '.state.json']:
        report.with_suffix(suffix).unlink(missing_ok=True)
    report.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix='rodtty-native-frame-') as directory:
        temp = Path(directory)
        env = os.environ.copy()
        env['RODTTY_HOME'] = str(temp / 'owner')
        # winit's selected native raw handle, not WINIT_UNIX_BACKEND, identifies backend.
        if args.backend == 'x11':
            env.pop('WAYLAND_DISPLAY', None)
            env.pop('WAYLAND_SOCKET', None)
        log = open(str(report) + '.log', 'w')
        app = subprocess.Popen([args.binary, '--frame-check', str(report), '--control-stdin', '--cwd', str(temp)], stdin=subprocess.PIPE, stdout=log, stderr=log, env=env, text=True)
        driver, compositor, previous = None, None, None
        evidence = {'passed': False, 'requested_backend': args.backend, 'input_driver': 'KWin Wayland fake-input' if args.fake_input_xml else 'XTest'}
        try:
            wait_for(lambda: report.with_suffix('.ready.json').exists() or app.poll() is not None, 'native frame did not become ready')
            assert app.poll() is None, f'Rodtty exited early; see {report}.log'
            compositor = KWin(app.pid)
            previous = compositor.query()
            initial = wait_for(lambda: compositor.query()['target'], 'isolated process has no compositor window')
            evidence['compositor_initial'] = initial
            ready = json.loads(report.with_suffix('.ready.json').read_text())
            evidence['initial_native_state'] = ready
            # KWin noBorder is a rule/override flag; CSD windows may report false
            # with zero native decoration extents. Compare the observed geometries.
            assert initial['frame'] == initial['client'], 'native frame extends outside app-drawn content'
            compositor.query('if(target) workspace.activeWindow = target;')
            driver = Input(args, temp)
            native_per_logical = ready['size'][0] / initial['client'][2]
            ui_per_logical = ready['scale'] / native_per_logical
            if args.backend == 'x11' and not driver.driver:
                driver.coordinate_scale = native_per_logical
                driver.offset = tuple(ready['position'][i] - initial['client'][i] * native_per_logical for i in range(2))

            def target():
                return compositor.query()['target']

            def state():
                path = report.with_suffix('.state.json')
                path.unlink(missing_ok=True)
                app.stdin.write('frame-status\n')
                app.stdin.flush()
                wait_for(path.exists, 'native state request timed out')
                return json.loads(path.read_text())

            def control(which):
                time.sleep(.6)  # Wait for the configured size to be presented before hit testing.
                f = target()['frame']
                # Frame padding 6, header padding 12, 32-wide controls with 2-point gaps.
                driver.click(f[0] + f[2] - {'close': 34, 'maximize': 68, 'minimize': 102}[which] * ui_per_logical, f[1] + 28 * ui_per_logical)

            def same_frame(left, right):
                return all(abs(a - b) <= .5 / native_per_logical for a, b in zip(left, right))

            original = target()['frame']
            control('maximize')
            wait_for(lambda: state()['maximized'], 'custom maximize button had no effect')
            control('maximize')
            wait_for(lambda: not state()['maximized'], 'custom restore button had no effect')
            restored = wait_for(lambda: (v if same_frame((v := target()['frame']), original) else None), 'restore did not recover original compositor geometry')
            time.sleep(.4)
            f = restored
            driver.click(f[0] + 230 * ui_per_logical, f[1] + 25 * ui_per_logical)
            driver.down()
            driver.up()
            wait_for(lambda: state()['maximized'], 'header double-click did not maximize')
            time.sleep(.5)
            f = target()['frame']
            driver.click(f[0] + 230 * ui_per_logical, f[1] + 25 * ui_per_logical)
            driver.down()
            driver.up()
            wait_for(lambda: not state()['maximized'], 'header double-click did not restore')
            time.sleep(.5)
            f = target()['frame']
            drag_before = f
            driver.move(f[0] + 230 * ui_per_logical, f[1] + 25 * ui_per_logical)
            time.sleep(.5)
            driver.down()
            driver.move(f[0] + 230 * ui_per_logical + 30, f[1] + 25 * ui_per_logical + 30)
            driver.up()
            moved = wait_for(lambda: (v if (v := target()['frame'])[:2] != f[:2] else None), 'header drag did not move window')
            sizes = []
            for name, fx, fy, dx, dy in [('east', 1, .5, 25, 0), ('south', .5, 1, 0, 25), ('southeast', 1, 1, 25, 25), ('northwest', 0, 0, -20, -20), ('north', .5, 0, 0, -15), ('west', 0, .5, -15, 0), ('northeast', 1, 0, 15, -15), ('southwest', 0, 1, -15, 15)]:
                f = target()['frame']
                x, y = f[0] + (f[2] - 3) * fx + (3 if fx == 0 else 0), f[1] + (f[3] - 3) * fy + (3 if fy == 0 else 0)
                driver.move(x, y)
                driver.down()
                driver.move(x + dx, y + dy)
                driver.up()
                resized = wait_for(lambda: (v if (v := target()['frame'])[2:] != f[2:] else None), f'{name} native resize had no effect')
                sizes.append({'edge': name, 'before': f, 'after': resized})
            control('minimize')
            wait_for(lambda: target()['minimized'], 'custom minimize button had no compositor effect')
            minimized = state()
            if args.backend == 'wayland':
                assert minimized['minimized'] is None, 'Wayland minimized state must remain unknown in winit'
            else:
                assert minimized['minimized'] is True, 'X11 native minimize acknowledgement missing'
            # Wayland has no xdg request to undo minimization. Restore through
            # the compositor, as taskbar activation does, without inventing a winit ack.
            compositor.query('if(target) { target.minimized = false; workspace.activeWindow = target; }')
            wait_for(lambda: not target()['minimized'], 'compositor activation did not restore minimized window')
            if args.screenshot:
                subprocess.run(['spectacle', '-b', '-n', '-a', '-o', args.screenshot], check=True, timeout=30)
            control('close')
            wait_for(lambda: state()['unsaved_prompt'], 'custom close bypassed native dirty-file decision')
            assert target() is not None, 'custom close destroyed the dirty editor window'
            if args.screenshot:
                prompt = str(Path(args.screenshot).with_suffix('.prompt.png'))
                subprocess.run(['spectacle', '-b', '-n', '-a', '-o', prompt], check=True, timeout=30)
            driver.send('key', 1, 1)  # evdev Escape; Xlib adapter maps it to keycode 9.
            driver.send('key', 1, 0)
            wait_for(lambda: not state()['unsaved_prompt'], 'Escape did not cancel native dirty close')
            assert state()['dirty'], 'canceling native custom close discarded the editor draft'
            evidence.update(passed=True, native_custom_close_prompt=True, native_cancel_kept_draft=True, undecorated=True, compositor_initial=initial,
                            maximize_restore=True, restore_geometry=restored, header_double_click=True,
                            header_drag={'before': drag_before, 'after': moved}, resize=sizes,
                            minimize_compositor_ack=True, minimized_native_state=minimized, minimize_restored_via='compositor activation')
        except Exception as error:
            evidence['error'] = str(error)
            if compositor:
                evidence['compositor_at_failure'] = compositor.query()['target']
            if report.with_suffix('.state.json').exists():
                evidence['native_at_failure'] = json.loads(report.with_suffix('.state.json').read_text())
        finally:
            if driver:
                if previous and previous.get('pointer'):
                    driver.move(*previous['pointer'])
                driver.close()
            if compositor and previous and previous.get('priorFocus'):
                identifier = json.dumps(previous['priorFocus'])
                compositor.query(f'const priorWindow = workspace.windowList().find(w=>String(w.internalId)==={identifier}); if(priorWindow) workspace.activeWindow=priorWindow;')
            report.with_suffix('.input.json').write_text(json.dumps(evidence, indent=2))
            try:
                code = app.wait(timeout=60)
            except subprocess.TimeoutExpired:
                app.terminate()
                code = app.wait(timeout=10)
            log.close()
        assert evidence['passed'], json.dumps(evidence, indent=2)
        assert code == 0, f'Lifecycle check failed; see {report}.log'
        output = json.loads(report.read_text())
        assert output['passed'], output
        expected_backend = 'Wayland' if args.backend == 'wayland' else 'Xlib'
        assert output['window_backend'] == expected_backend, output
        print(json.dumps(output, indent=2))


if __name__ == '__main__':
    main()
