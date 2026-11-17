#!/usr/bin/env python3
"""Native Windows custom-frame input and session lifecycle check.

Uses Win32 geometry/state as independent evidence and SendInput for actual mouse
and keyboard events. Run in an unlocked interactive Windows desktop at the same
integrity level as Rodtty. Linux metadata compilation is not native execution.
"""
import argparse
import ctypes
from ctypes import wintypes as W
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time


def wait_for(read, message, timeout=20):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        value = read()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def same(a, b):
    return all(abs(x - y) <= 1 for x, y in zip(a, b))


class Windows:
    def __init__(self, pid):
        self.pid = pid
        self.user = ctypes.WinDLL('user32', use_last_error=True)
        self.dwm = ctypes.WinDLL('dwmapi', use_last_error=True)
        self.callback_type = ctypes.WINFUNCTYPE(W.BOOL, W.HWND, W.LPARAM)
        signatures = {
            'EnumWindows': ([self.callback_type, W.LPARAM], W.BOOL),
            'GetWindowThreadProcessId': ([W.HWND, ctypes.POINTER(W.DWORD)], W.DWORD),
            'IsWindowVisible': ([W.HWND], W.BOOL), 'IsIconic': ([W.HWND], W.BOOL),
            'IsZoomed': ([W.HWND], W.BOOL),
            'GetWindowRect': ([W.HWND, ctypes.POINTER(W.RECT)], W.BOOL),
            'GetClientRect': ([W.HWND, ctypes.POINTER(W.RECT)], W.BOOL),
            'ClientToScreen': ([W.HWND, ctypes.POINTER(W.POINT)], W.BOOL),
            'SetCursorPos': ([ctypes.c_int, ctypes.c_int], W.BOOL),
            'GetCursorPos': ([ctypes.POINTER(W.POINT)], W.BOOL),
            'GetForegroundWindow': ([], W.HWND),
            'SetForegroundWindow': ([W.HWND], W.BOOL),
            'GetWindowLongW': ([W.HWND, ctypes.c_int], W.LONG),
            'ShowWindow': ([W.HWND, ctypes.c_int], W.BOOL),
            'SetProcessDpiAwarenessContext': ([W.HANDLE], W.BOOL),
        }
        for name, (arguments, result) in signatures.items():
            fn = getattr(self.user, name)
            fn.argtypes, fn.restype = arguments, result
        self.dwm.DwmGetWindowAttribute.argtypes = [W.HWND, W.DWORD, W.LPVOID, W.DWORD]
        self.dwm.DwmGetWindowAttribute.restype = ctypes.c_long
        # Keep all queried/input coordinates in physical pixels, including on
        # mixed-DPI desktops. This process has not created any windows.
        if not self.user.SetProcessDpiAwarenessContext(W.HANDLE(-4)):
            raise ctypes.WinError(ctypes.get_last_error())
        ulong_ptr = ctypes.c_size_t

        class Mouse(ctypes.Structure):
            _fields_ = [('dx', W.LONG), ('dy', W.LONG), ('mouseData', W.DWORD),
                        ('dwFlags', W.DWORD), ('time', W.DWORD), ('dwExtraInfo', ulong_ptr)]

        class Keyboard(ctypes.Structure):
            _fields_ = [('wVk', W.WORD), ('wScan', W.WORD), ('dwFlags', W.DWORD),
                        ('time', W.DWORD), ('dwExtraInfo', ulong_ptr)]

        class Payload(ctypes.Union):
            _fields_ = [('mouse', Mouse), ('keyboard', Keyboard)]

        class Input(ctypes.Structure):
            _fields_ = [('type', W.DWORD), ('payload', Payload)]

        self.Input = Input
        self.user.SendInput.argtypes = [W.UINT, ctypes.POINTER(Input), ctypes.c_int]
        self.user.SendInput.restype = W.UINT
        self.prior_focus = self.user.GetForegroundWindow()
        self.prior_pointer = W.POINT()
        assert self.user.GetCursorPos(ctypes.byref(self.prior_pointer))

    def window(self):
        found = []
        @self.callback_type
        def visit(hwnd, _):
            pid = W.DWORD()
            self.user.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == self.pid and self.user.IsWindowVisible(hwnd):
                found.append(hwnd)
            return True
        assert self.user.EnumWindows(visit, 0)
        assert len(found) <= 1, f'Expected one visible window for launched PID: {found}'
        return found[0] if found else None

    def query(self):
        hwnd = self.window()
        if not hwnd:
            return None
        outer, client, visible = W.RECT(), W.RECT(), W.RECT()
        origin = W.POINT()
        assert self.user.GetWindowRect(hwnd, ctypes.byref(outer))
        assert self.user.GetClientRect(hwnd, ctypes.byref(client))
        assert self.user.ClientToScreen(hwnd, ctypes.byref(origin))
        result = self.dwm.DwmGetWindowAttribute(hwnd, 9, ctypes.byref(visible), ctypes.sizeof(visible))
        assert result == 0, f'DwmGetWindowAttribute: {result}'
        rect = lambda r: [r.left, r.top, r.right - r.left, r.bottom - r.top]
        return {'outer': rect(outer), 'visible': rect(visible),
                'client': [origin.x, origin.y, client.right, client.bottom],
                'minimized': bool(self.user.IsIconic(hwnd)),
                'maximized': bool(self.user.IsZoomed(hwnd)),
                'style': self.user.GetWindowLongW(hwnd, -16) & 0xffffffff}

    def send(self, *, mouse=None, key=None, up=False):
        value = self.Input()
        if mouse is not None:
            value.payload.mouse.dwFlags = mouse
        else:
            value.type = 1
            value.payload.keyboard.wVk = key
            value.payload.keyboard.dwFlags = 2 if up else 0
        assert self.user.SendInput(1, ctypes.byref(value), ctypes.sizeof(value)) == 1, 'SendInput rejected input'

    def move(self, x, y):
        assert self.user.SetCursorPos(round(x), round(y))
        time.sleep(.05)

    def click(self, x, y, double=False):
        self.move(x, y)
        for _ in range(2 if double else 1):
            self.send(mouse=2)
            self.send(mouse=4)
            time.sleep(.06)
        time.sleep(.25)

    def drag(self, x, y, dx, dy):
        self.move(x, y)
        time.sleep(.2)
        self.send(mouse=2)
        try:
            for step in range(1, 11):
                self.move(x + dx * step / 10, y + dy * step / 10)
        finally:
            self.send(mouse=4)
        time.sleep(.35)

    def restore_prior(self):
        self.send(mouse=4)
        self.move(self.prior_pointer.x, self.prior_pointer.y)
        if self.prior_focus:
            self.user.SetForegroundWindow(self.prior_focus)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    args = parser.parse_args()
    assert platform.system() == 'Windows', 'Native Windows GUI host required'
    assert 'TMPDIR' in os.environ, 'Set TMPDIR under target/ on the binary filesystem'
    binary, report = args.binary.resolve(), args.report.resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    evidence = {'passed': False, 'os': platform.platform(), 'runtime': 'native Win32/SendInput'}
    # Python uses TEMP on Windows; explicitly honor the same isolated fixture root.
    with tempfile.TemporaryDirectory(prefix='rodtty-windows-frame-', dir=os.environ['TMPDIR']) as folder:
        env = dict(os.environ, RODTTY_HOME=str(Path(folder) / 'owner'))
        for suffix in ['.json', '.ready.json', '.state.json', '.input.json']:
            report.with_suffix(suffix).unlink(missing_ok=True)
        with report.with_suffix('.log').open('w') as log:
            app = subprocess.Popen([str(binary), '--frame-check', str(report), '--control-stdin', '--cwd', folder],
                                   env=env, stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)
            driver = None
            def state():
                path = report.with_suffix('.state.json')
                path.unlink(missing_ok=True)
                app.stdin.write('frame-status\n')
                app.stdin.flush()
                wait_for(path.exists, 'Native state request timed out')
                return json.loads(path.read_text())
            def frame():
                return driver.query()['client']
            def control(name):
                time.sleep(.4)
                f = frame()
                scale = state()['scale']
                driver.click(f[0] + f[2] - {'close': 34, 'maximize': 68, 'minimize': 102}[name] * scale,
                             f[1] + 28 * scale)
            try:
                driver = Windows(app.pid)
                wait_for(lambda: report.with_suffix('.ready.json').exists() or app.poll() is not None,
                         'Frame fixture did not become ready', 45)
                assert app.poll() is None, f'Rodtty exited early; see {report.with_suffix(".log")}'
                ready = json.loads(report.with_suffix('.ready.json').read_text())
                assert ready['decorated'] is False, ready
                wait_for(driver.window, 'No visible native window for launched PID')
                assert driver.user.SetForegroundWindow(driver.window()), 'Cannot focus launched window'
                initial = driver.query()
                assert initial['style'] & 0x40000, 'Native sizing/AeroSnap style was removed'
                assert same(initial['outer'], initial['client']), 'Native non-client border remains'
                assert same(initial['visible'], initial['client']), 'Visible DWM frame remains outside app content'
                original = frame()
                control('maximize')
                wait_for(lambda: state()['maximized'] and driver.query()['maximized'], 'Custom maximize had no effect')
                maximum = frame()
                driver.drag(maximum[0] + 3, maximum[1] + 3, 20, 20)
                assert state()['maximized'] and same(frame(), maximum), 'Maximized resize band active'
                control('maximize')
                wait_for(lambda: not state()['maximized'] and same(frame(), original), 'Restore lost original geometry')
                for maximized in [True, False]:
                    f, scale = frame(), state()['scale']
                    driver.click(f[0] + 230 * scale, f[1] + 25 * scale, double=True)
                    wait_for(lambda: state()['maximized'] == maximized, 'Header double-click did not toggle maximize')
                before, scale = frame(), state()['scale']
                driver.drag(before[0] + 230 * scale, before[1] + 25 * scale, 25, 25)
                moved = wait_for(lambda: (f if (f := frame())[:2] != before[:2] else None), 'Header drag had no effect')
                evidence['header_drag'] = {'before': before, 'after': moved}
                # A pressed control is excluded from the header's move region.
                f = frame()
                driver.drag(f[0] + f[2] - 68 * scale, f[1] + 28 * scale, -60 * scale, 0)
                assert same(frame(), f), 'Header child started native window movement'
                evidence['resize'] = []
                for name, fx, fy, dx, dy in [
                    ('east', 1, .5, 20, 0), ('south', .5, 1, 0, 20), ('southeast', 1, 1, 20, 20),
                    ('northwest', 0, 0, -20, -20), ('north', .5, 0, 0, -20), ('west', 0, .5, -20, 0),
                    ('northeast', 1, 0, 20, -20), ('southwest', 0, 1, -20, 20)]:
                    f = frame()
                    x, y = f[0] + max(3, (f[2] - 3) * fx), f[1] + max(3, (f[3] - 3) * fy)
                    driver.drag(x, y, dx, dy)
                    after = wait_for(lambda: (v if (v := frame())[2:] != f[2:] else None), name + ' resize had no effect')
                    assert abs(after[0] + after[2] - f[0] - f[2]) <= 1 if fx == 0 else abs(after[0] - f[0]) <= 1
                    assert abs(after[1] + after[3] - f[1] - f[3]) <= 1 if fy == 0 else abs(after[1] - f[1]) <= 1
                    evidence['resize'].append({'edge': name, 'before': f, 'after': after})
                f, scale = frame(), state()['scale']
                driver.drag(f[0] + 3, f[1] + 3, f[2] - 80, f[3] - 80)
                small = frame()
                assert same(small[2:], [640 * scale, 360 * scale]), f'Minimum-size clamp: {small}'
                assert same([small[0] + small[2], small[1] + small[3]], [f[0] + f[2], f[1] + f[3]])
                control('minimize')
                wait_for(lambda: state()['minimized'] and driver.query()['minimized'], 'Custom minimize had no native acknowledgement')
                driver.user.ShowWindow(driver.window(), 9)  # SW_RESTORE, as taskbar activation does.
                driver.user.SetForegroundWindow(driver.window())
                wait_for(lambda: not state()['minimized'], 'Taskbar-style restore failed')
                control('close')
                wait_for(lambda: state()['unsaved_prompt'], 'Custom close bypassed dirty-file prompt')
                assert driver.window(), 'Custom close destroyed dirty editor window'
                driver.send(key=0x1B)
                driver.send(key=0x1B, up=True)
                wait_for(lambda: not state()['unsaved_prompt'], 'Escape did not cancel custom close')
                assert state()['dirty'], 'Close Cancel discarded the draft'
                evidence.update(passed=True, undecorated=True, maximize_restore=True, header_double_click=True,
                                header_child_non_drag=True, minimized_native_ack=True, minimum_size=small,
                                native_custom_close_prompt=True, native_cancel_kept_draft=True, initial=initial)
            except Exception as error:
                evidence['error'] = str(error)
            finally:
                report.with_suffix('.input.json').write_text(json.dumps(evidence, indent=2))
                if driver:
                    driver.send(mouse=4)
                try:
                    code = app.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    app.terminate()
                    code = app.wait(timeout=10)
                finally:
                    if driver:
                        driver.restore_prior()
            assert evidence['passed'], evidence
            assert code == 0, f'Lifecycle failed; see {report.with_suffix(".log")}'
            result = json.loads(report.read_text())
            assert result['passed'] and result['window_backend'] == 'Win32', result
            print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
