#!/usr/bin/env python3
"""Drive the actual macOS custom frame and the existing session/dirty-close harness.
Requires a macOS GUI login, Xcode command-line tools, and Accessibility permission
for the invoking terminal. Run on the Mac; Linux compilation is not execution.
"""
import argparse
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    args = parser.parse_args()
    assert platform.system() == 'Darwin', 'Native macOS GUI host required'
    binary, report = args.binary.resolve(), args.report.resolve()
    report.parent.mkdir(parents=True, exist_ok=True)
    assert 'TMPDIR' in os.environ, 'Set TMPDIR under target/ on the binary filesystem'
    evidence = {'passed': False, 'os': platform.platform(), 'runtime': 'native AppKit/Quartz'}
    with tempfile.TemporaryDirectory(prefix='rodtty-mac-frame-') as folder:
        temp = Path(folder)
        helper = temp / 'mac-frame-input'
        subprocess.run(['swiftc', str(Path(__file__).with_name('mac-frame-input.swift')), '-o', str(helper)], check=True)
        env = dict(os.environ, RODTTY_HOME=str(temp / 'owner'))
        for suffix in ['.json', '.ready.json', '.state.json', '.input.json']:
            report.with_suffix(suffix).unlink(missing_ok=True)
        with report.with_suffix('.log').open('w') as log:
            app = subprocess.Popen([str(binary), '--frame-check', str(report), '--control-stdin'],
                                   env=env, stdin=subprocess.PIPE, stdout=log, stderr=log, text=True)
            driver = subprocess.Popen([str(helper), str(app.pid)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            def command(action, **values):
                driver.stdin.write(json.dumps(dict(action=action, **values)) + '\n')
                driver.stdin.flush()
                value = json.loads(driver.stdout.readline())
                assert 'error' not in value, value
                return value
            def state():
                path = report.with_suffix('.state.json')
                path.unlink(missing_ok=True)
                app.stdin.write('frame-status\n')
                app.stdin.flush()
                wait_for(path.exists, 'UI native state timed out')
                return json.loads(path.read_text())
            def frame():
                return command('query').get('frame')
            def click(x, y, double=False):
                command('move', x=x, y=y)
                for count in range(1, 3 if double else 2):
                    command('down', x=x, y=y, clicks=count)
                    command('up', x=x, y=y, clicks=count)
                    if double:
                        time.sleep(.06)
                time.sleep(.25)
            def drag(x, y, dx, dy):
                command('move', x=x, y=y)
                time.sleep(.2)
                command('down', x=x, y=y)
                for step in range(1, 11):
                    command('move', x=x + dx * step / 10, y=y + dy * step / 10)
                    time.sleep(.025)
                command('up', x=x + dx, y=y + dy)
                time.sleep(.35)
            def control(name):
                f = frame()
                # App-drawn 32-point controls: close, minimize, maximize; no OS lights.
                click(f[0] + {'close': 34, 'minimize': 68, 'maximize': 102}[name], f[1] + 28)
            try:
                assert command('access')['trusted'], 'Grant Accessibility to the invoking terminal, then rerun'
                wait_for(report.with_suffix('.ready.json').exists, 'Frame fixture did not become ready', 45)
                ready = json.loads(report.with_suffix('.ready.json').read_text())
                assert ready['decorated'] is False, ready
                command('activate')
                original = wait_for(frame, 'No native window for launched PID')
                control('maximize')
                wait_for(lambda: state()['maximized'], 'Custom maximize had no effect')
                maximum = frame()
                drag(maximum[0] + 3, maximum[1] + 3, 20, 20)
                assert state()['maximized'] and same(frame(), maximum), 'Maximized resize band active'
                control('maximize')
                wait_for(lambda: not state()['maximized'] and same(frame(), original), 'Restore lost original geometry')
                for maximized in [True, False]:
                    f = frame()
                    click(f[0] + 340, f[1] + 25, double=True)
                    wait_for(lambda: state()['maximized'] == maximized, 'Header double-click did not toggle maximize')
                    assert not state()['minimized'], 'Header double-click followed the OS minimize preference'
                before = frame()
                drag(before[0] + 340, before[1] + 25, 25, 25)
                moved = wait_for(lambda: (f if (f := frame())[:2] != before[:2] else None), 'Header drag had no effect')
                evidence['header_drag'] = {'before': before, 'after': moved}
                evidence['resize'] = []
                for name, fx, fy, dx, dy in [
                    ('east', 1, .5, 20, 0), ('south', .5, 1, 0, 20), ('southeast', 1, 1, 20, 20),
                    ('northwest', 0, 0, -20, -20), ('north', .5, 0, 0, -20), ('west', 0, .5, -20, 0),
                    ('northeast', 1, 0, 20, -20), ('southwest', 0, 1, -20, 20)]:
                    f = frame()
                    x, y = f[0] + max(3, (f[2] - 3) * fx), f[1] + max(3, (f[3] - 3) * fy)
                    drag(x, y, dx, dy)
                    after = wait_for(lambda: (v if (v := frame())[2:] != f[2:] else None), name + ' resize had no effect')
                    assert abs(after[0] + after[2] - f[0] - f[2]) <= 1 if fx == 0 else abs(after[0] - f[0]) <= 1
                    assert abs(after[1] + after[3] - f[1] - f[3]) <= 1 if fy == 0 else abs(after[1] - f[1]) <= 1
                    evidence['resize'].append({'edge': name, 'before': f, 'after': after})
                # Deliberately drag through the minimum; the far edges must remain fixed.
                f = frame()
                drag(f[0] + 3, f[1] + 3, f[2] - 80, f[3] - 80)
                small = frame()
                assert abs(small[2] - 640) <= 1 and abs(small[3] - 360) <= 1, small
                assert same([small[0] + small[2], small[1] + small[3]], [f[0] + f[2], f[1] + f[3]])
                control('minimize')
                wait_for(lambda: state()['minimized'], 'Custom minimize had no native acknowledgement')
                command('restore')
                wait_for(lambda: not state()['minimized'], 'Dock-style restore failed')
                control('close')
                wait_for(lambda: state()['unsaved_prompt'], 'Custom close bypassed dirty-file prompt')
                command('escape')
                wait_for(lambda: not state()['unsaved_prompt'], 'Escape did not cancel custom close')
                assert state()['dirty'], 'Close Cancel discarded the draft'
                evidence.update(passed=True, undecorated=True, maximize_restore=True, header_double_click=True,
                                minimized_native_ack=True, minimum_size=small, native_custom_close_prompt=True,
                                native_cancel_kept_draft=True, initial=ready)
            except Exception as error:
                evidence['error'] = str(error)
            finally:
                report.with_suffix('.input.json').write_text(json.dumps(evidence, indent=2))
                try:
                    command('restorePrior')
                except Exception:
                    pass
                driver.terminate()
                driver.wait(timeout=5)
                try:
                    code = app.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    app.terminate()
                    code = app.wait(timeout=10)
            assert evidence['passed'], evidence
            assert code == 0, f'Lifecycle failed; see {report.with_suffix(".log")}'
            result = json.loads(report.read_text())
            assert result['passed'] and result['window_backend'] == 'AppKit', result
            print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
