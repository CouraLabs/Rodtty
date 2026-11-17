#!/usr/bin/env python3
"""Capture the current desktop matrix without writing fixture files or Git index.

Requires Pillow, a built Rodtty, and isolated storage on the build filesystem.
The synthetic terminal/editor scenes are explicitly recorded as sample content;
all real-diff variants read HEAD/index/disk through the production Git worker.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

FAMILIES = ['catppuccin', 'dracula', 'tokyo_night', 'nord', 'gruvbox',
            'one_dark_pro', 'night_owl', 'ayu', 'solarized']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--storage', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--resume', action='store_true', help='Reuse this harness’s own fixture and unchanged frames')
    parser.add_argument('--only-tree', action='store_true', help='Refresh Tree frames after a tooling-only scene fix; retain per-frame source provenance')
    args = parser.parse_args()
    if args.only_tree and not args.resume:
        parser.error('--only-tree requires --resume and an existing matrix')
    source = subprocess.check_output(['git', 'rev-parse', 'HEAD']).decode().strip()
    binary_hash = hashlib.sha256(args.binary.read_bytes()).hexdigest()
    for path in [args.storage, args.output]:
        path.mkdir(parents=True, exist_ok=True)
    fixture = args.storage / 'git-fixture'
    fresh = not fixture.exists()
    if not fresh and not args.resume:
        raise SystemExit('Use fresh --storage or explicit --resume')
    fixture.mkdir(exist_ok=True)
    env = dict(os.environ, GIT_OPTIONAL_LOCKS='0', RUNE_PERF_WARN='0')
    def git(*words):
        return subprocess.check_output(['git', '-C', str(fixture), '-c', 'user.name=Fixture',
                                       '-c', 'user.email=fixture@example.test',
                                       '-c', 'commit.gpgsign=false', *words], env=env)
    if fresh:
        git('init', '-b', 'main')
        (fixture / 'src/desktop').mkdir(parents=True)
        (fixture / 'client').mkdir()
        for path, text in {'a-main.rs': 'fn main() {\n    println!("HEAD version");\n}\n',
                           'src/desktop/workspace.rs': '// nested workspace\nfn render() {}\n',
                           'client/a-main.rs': '// duplicate basename\n',
                           '.gitignore': 'ignored/\n', '.hidden.txt': 'hidden fixture\n',
                           'package.json': '{"name":"fixture"}\n'}.items():
            (fixture / path).write_text(text)
        git('add', '.')
        git('commit', '-m', 'qualification fixture')
        (fixture / 'a-main.rs').write_text('fn main() {\n    println!("INDEX version");\n}\n')
        git('add', 'a-main.rs')
        (fixture / 'a-main.rs').write_text('fn main() {\n    // Saved worktree change\n    println!("DISK version 日本語");\n}\n')
        (fixture / 'untracked.rs').write_text('// untracked addition\n')
        (fixture / 'ignored').mkdir()
        (fixture / 'ignored/cache.txt').write_text('ignored fixture\n')
    def versions():
        paths = sorted(p for p in fixture.rglob('*') if p.is_file() and '.git' not in p.relative_to(fixture).parts)
        return {'files': {str(p.relative_to(fixture)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
                'index_sha256': hashlib.sha256((fixture / '.git/index').read_bytes()).hexdigest(),
                'status': git('status', '--porcelain=v1', '--untracked-files=all').decode()}
    before = versions()
    prior_manifest = args.output / "matrix.json"
    prior_rows = {}
    if args.only_tree and not prior_manifest.exists():
        parser.error('--only-tree requires an existing matrix')
    if args.resume and prior_manifest.exists():
        prior = json.loads(prior_manifest.read_text())
        assert before == prior["fixture_before"], "Fixture changed since first capture"
        if not args.only_tree:
            assert binary_hash == prior["binary_sha256"], "Binary changed; capture into fresh storage/output"
        prior_rows = {row["file"]: {**row, "source_commit": row.get("source_commit", prior["source_commit"]), "binary_sha256": row.get("binary_sha256", prior["binary_sha256"])} for row in prior["frames"]}
    scripts = args.storage / 'scripts'
    scripts.mkdir(exist_ok=True)
    def tap(key, t=.2): return {'t': t, 'type': 'tap', 'key': key}
    variants = {
        'terminal': ('preview', []),
        'tree': ('tree', []),
        'editor-selection': ('workspace', [tap('file-editor-1'), {'t': .4, 'type': 'key', 'name': 'a', 'cmd': True}]),
        'settings': ('settings', [tap('settings-search'), {'t': .4, 'type': 'type', 'text': 'theme', 'every': .01}, {'t': .8, 'type': 'key', 'name': 'Tab'}]),
        'settings-appearance': ('settings', [tap('settings-search'), {'t': .4, 'type': 'type', 'text': 'theme', 'every': .01}, {'t': .65, 'type': 'scroll', 'key': 'settings-list', 'dy': -280, 'dur': .3}]),
        'real-current': ('real-diff', []),
        'real-unstaged': ('real-diff', [tap('diff-unstaged')]),
        'real-staged': ('real-diff', [tap('diff-staged')]),
        'real-split': ('real-diff', [tap('diff-split')]),
        'real-inline': ('real-diff', [tap('diff-inline')]),
        'palette': ('palette', []),
    }
    rows = []
    for family in FAMILIES:
        for mode in ['light', 'dark']:
            owner = args.storage / f'{family}-{mode}'
            owner.mkdir(exist_ok=True)
            (owner / 'settings.json').write_text(json.dumps({'theme_family': family, 'appearance': mode}))
            for width, height in [(1440, 900), (640, 360)]:
                for variant, (scene, events) in variants.items():
                    name = f'{family}-{mode}-{width}x{height}-{variant}.png'
                    output = args.output / name
                    recapture = (variant == 'tree' and width == 640) or (variant == 'real-current' and width == 1440)
                    events = list(events)
                    if variant == 'tree' and width == 640:
                        events = [tap('files')]
                    if variant == 'real-current' and width == 1440:
                        events = [tap('toggle-terminal')]
                    command = [str(args.binary), '--cwd', str(fixture), '--scene', scene, '--size', f'{width}x{height}',
                               '--scale', '1', '--at', '1.4', '--snap', str(output)]
                    if events:
                        script = scripts / f'{variant}.json'
                        script.write_text(json.dumps(events))
                        command += ['--script', str(script)]
                    result = subprocess.CompletedProcess(command, 0, b'', b'')
                    render = (not args.resume or not output.exists() or recapture) if not args.only_tree else variant == "tree"
                    if render:
                        result = subprocess.run(command, env=dict(env, RODTTY_HOME=str(owner)), capture_output=True)
                    if result.returncode:
                        raise RuntimeError(f'{name}: {result.stderr.decode()}')
                    warnings = [line for line in result.stderr.decode().splitlines() if 'not found' in line or 'missing' in line.lower()]
                    if warnings:
                        raise RuntimeError(f'{name}: {warnings}')
                    rows.append({'family': family, 'appearance': mode, 'width': width, 'height': height,
                                 'scene': scene, 'variant': variant, 'file': name,
                                 'sha256': hashlib.sha256(output.read_bytes()).hexdigest(),
                                 'source_commit': source if render else prior_rows[name]['source_commit'],
                                 'binary_sha256': binary_hash if render else prior_rows[name]['binary_sha256']})
            print(f'{family} {mode}: captured', flush=True)
    after = versions()
    assert before == after, 'Render inspection changed fixture files/index/status'
    manifest = {'source_commit': source, 'binary_sha256': binary_hash, 'capture_sources': sorted({row['source_commit'] for row in rows}),
                'frames': rows, 'fixture_before': before, 'fixture_after': after, 'files_index_status_unchanged': True,
                'sample_scenes': ['preview', 'workspace', 'palette'], 'wide_real_current': 'Terminal hidden to exercise automatic split; other default three-pane comparisons may remain inline', 'real_git_scenes': ['real-diff'],
                'live_system': 'Covered by app runtime tests; these frames are explicit Light/Dark captures'}
    (args.output / 'matrix.json').write_text(json.dumps(manifest, indent=2) + '\n')
    from PIL import Image, ImageDraw
    for variant in variants:
        for width, height in [(1440, 900), (640, 360)]:
            thumb = (480, 300) if width == 1440 else (480, 270)
            sheet = Image.new('RGB', (thumb[0] * 2, (thumb[1] + 24) * 9), '#d0d0d0')
            draw = ImageDraw.Draw(sheet)
            for row, family in enumerate(FAMILIES):
                for col, mode in enumerate(['light', 'dark']):
                    frame = Image.open(args.output / f'{family}-{mode}-{width}x{height}-{variant}.png').convert('RGB')
                    frame.thumbnail(thumb)
                    x, y = col * thumb[0], row * (thumb[1] + 24)
                    draw.text((x + 5, y + 5), f'{family} {mode} {width}x{height} {variant}', fill='black')
                    sheet.paste(frame, (x, y + 24))
            sheet.save(args.output / f'contact-{width}-{variant}.jpg', quality=88)
    print(f'{len(rows)} frames; fixture file/index/status invariants passed', flush=True)


if __name__ == '__main__':
    main()
