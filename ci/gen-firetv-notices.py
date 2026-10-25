#!/usr/bin/env python3
"""Render the Rust crate tables of THIRD-PARTY-NOTICES-FIRETV.md from the lockfile and `cargo tree`.

The Fire TV APK links the crates of `plx_android` for armv7-linux-androideabi: most are the webOS app's, whose
licence and copyright rows live in `ci/third-party-crates.json` (`groups`, `std_lock`), plus the ones only the APK
links (`firetv_crates`). Versions come from `rust-modules/Cargo.lock` and the std snapshot, filtered to the
versions the Android graph resolves.

    python3 ci/gen-firetv-notices.py            rewrite the generated regions in place
    python3 ci/gen-firetv-notices.py --check    fail (exit 1) if the file differs or the data file disagrees with the graph

Needs cargo and the toolchain pinned in rust-modules/rust-toolchain.toml (`cargo tree` resolves; it compiles nothing).
EXIT CODES. 0: ok. 1: stale or inconsistent. 2: a crate in the Android graph has no row anywhere: a licence decision
only a person can make; one `unclassified: a b c` line is printed.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location('gen_notices', ROOT / 'ci' / 'gen-third-party-notices.py')
base = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(base)
NOTICES = ROOT / 'THIRD-PARTY-NOTICES-FIRETV.md'
REGENERATE = 'python3 ci/gen-firetv-notices.py'


def android_tree():
    """{crate: {versions}} of the registry crates `plx_android` links for armv7-linux-androideabi."""
    cmd = ['cargo', '+' + base.pinned(ROOT), 'tree', '--locked', '--manifest-path', str(ROOT / 'rust-modules' / 'Cargo.toml'),
           '-p', 'plx_android', '--target', 'armv7-linux-androideabi', '--edges', 'normal,no-proc-macro', '--prefix', 'none',
           '--format', '{p}']
    out = subprocess.run(cmd, capture_output=True, text=True, env={**os.environ, 'CARGO_INCREMENTAL': '0'})
    if out.returncode != 0:
        raise SystemExit(f'cargo tree failed: {out.stderr.strip()[-400:]}')
    tree = {}
    for line in out.stdout.splitlines():
        m = re.match(r'(\S+) v(\S+)(?: \((.*)\))?$', line.strip())
        if m and not m.group(3):
            tree.setdefault(m.group(1), set()).add(m.group(2))
    return tree


def groups_for(data, tree):
    """The webOS groups cut down to the Android graph, with the APK-only crates added to the elected-MIT table."""
    std = set(data['std_lock'])
    out = []
    for group in data['groups']:
        keep = [c for c in group['crates'] if c['name'] in tree or c['name'] in std]
        if group['id'] == 'elected-mit':
            keep += data['firetv_crates']
        out.append({**group, 'crates': keep})
    return out


def problems_in(data, tree):
    problems = []
    rows = {c['name'] for g in data['groups'] for c in g['crates']}
    extra = {c['name'] for c in data['firetv_crates']}
    for name in sorted(extra & rows):
        problems.append(f'{name} is in both "firetv_crates" and a webOS group.')
    for name in sorted(extra - set(tree)):
        problems.append(f'"firetv_crates" lists {name}, which the Android graph no longer links: drop it.')
    return problems


def unclassified(data, tree):
    known = {c['name'] for g in data['groups'] for c in g['crates']} | {c['name'] for c in data['firetv_crates']}
    return sorted(n for n in tree if not n.startswith('plx_') and n not in known)


def region(gid):
    return (f'<!-- BEGIN GENERATED firetv:{gid} (edit ci/third-party-crates.json, run {REGENERATE}) -->',
            f'<!-- END GENERATED firetv:{gid} -->')


def render(text, data, project, tree):
    for group in groups_for(data, tree):
        # Registry versions the Android graph does not resolve are left out; std crates keep the snapshot's.
        scoped = {n: [v for v in project.get(n, ()) if v in tree.get(n, ())] for n in {c['name'] for c in group['crates']}}
        begin, end = region(group['id'])
        pattern = re.compile(re.escape(begin) + r'\n.*?\n' + re.escape(end), re.S)
        if not pattern.search(text):
            raise SystemExit(f'THIRD-PARTY-NOTICES-FIRETV.md has no "{begin}" ... "{end}" region for group {group["id"]}')
        body = f'{begin}\n\n{base.render_group(group, scoped, data)}\n\n{end}'
        text = pattern.sub(lambda _m: body, text, count=1)
    return text


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--check', action='store_true', help='fail instead of rewriting when the file would change')
    args = ap.parse_args(argv)
    data = json.loads(base.DATA.read_text(encoding='utf8'))
    project = base.parse_lock(base.LOCK.read_text(encoding='utf8'))
    tree = android_tree()
    problems = problems_in(data, tree)
    new = unclassified(data, tree)
    for name in new:
        problems.append(f'{name} {sorted(tree[name])} is in the Android graph but has no row in ci/third-party-crates.json '
                        f'("groups" for a webOS crate, "firetv_crates" for an APK-only one): a licence and copyright decision.')
    if problems:
        print('gen-firetv-notices: FAIL')
        for p in problems:
            print(f'  {p}')
        if new:
            print('unclassified: ' + ' '.join(new))
            return 2
        return 1
    current = NOTICES.read_text(encoding='utf8')
    wanted = render(current, data, project, tree)
    if wanted == current:
        print('gen-firetv-notices: ok (the Rust tables match the Android graph and rust-modules/Cargo.lock)')
        return 0
    if args.check:
        print('gen-firetv-notices: FAIL')
        print(f'  THIRD-PARTY-NOTICES-FIRETV.md no longer matches the Android graph. Run `{REGENERATE}`, review the diff, commit it.')
        return 1
    NOTICES.write_text(wanted, encoding='utf8', newline='\n')
    print('gen-firetv-notices: rewrote THIRD-PARTY-NOTICES-FIRETV.md')
    return 0


if __name__ == '__main__':
    sys.exit(main())
