#!/usr/bin/env python3
"""Check inline local Markdown links in non-ignored working-tree Markdown."""

import argparse
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit


LINK = re.compile(
    r'!?\[(?:\\.|[^\]\\\n])*\]\(\s*'
    r'(?P<target><[^>\n]*>|(?:\\.|[^\s()\\]|\((?:\\.|[^()\\])*\))+)'
    r'(?:\s+(?:"[^"\n]*"|\'[^\'\n]*\'|\([^()\n]*\)))?\s*\)'
)
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")


def local_links(text):
    """Yield (line number, decoded path), ignoring fenced code and URLs."""
    fence_char = None
    fence_length = 0
    for number, line in enumerate(text.splitlines(), 1):
        match = FENCE.match(line)
        if fence_char:
            if (match and match[1][0] == fence_char
                    and len(match[1]) >= fence_length and not match[2].strip()):
                fence_char = None
            continue
        if match:
            fence_char, fence_length = match[1][0], len(match[1])
            continue
        # Inline code is not a link either; preserve the surrounding text.
        line = re.sub(r'(`+).*?\1', '', line)
        for match in LINK.finditer(line):
            target = match['target']
            if target.startswith('<'):
                target = target[1:-1]
            target = re.sub(r'\\([!"#$%&\'()*+,\-./:;<=>?@\[\]^_`{|}~\\])', r'\1', target)
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            yield number, unquote(parsed.path)


def tracked_markdown(root):
    result = subprocess.run(
        ['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*.md', '*.markdown',
         ':!:target/**', ':!:**/target/**'],
        cwd=root, check=True, stdout=subprocess.PIPE,
    )
    # Include new docs and omit unstaged deletions so moves can be checked before staging.
    return sorted({Path(name) for name in result.stdout.decode('utf-8').split('\0')
                   if name and (root / name).is_file()})


def selected_markdown(root, names):
    """Validate explicit repository-relative sources without a Git discovery process."""
    sources = set()
    for name in names:
        source = Path(name)
        if source.is_absolute() or not (root / source).resolve().is_relative_to(root.resolve()):
            raise ValueError(f'expected a repository-relative Markdown file: {name}')
        if source.suffix not in ('.md', '.markdown') or not (root / source).is_file():
            raise ValueError(f'expected an existing Markdown file: {name}')
        sources.add(source)
    return sorted(sources)


def check(root, sources=None):
    errors = []
    for source in tracked_markdown(root) if sources is None else sources:
        for number, target in local_links((root / source).read_text(encoding='utf-8')):
            if not (root / source.parent / target).exists():
                errors.append(f'{source.as_posix()}:{number}: missing target: {target}')
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('files', nargs='*', help=(
        'repository-relative Markdown sources; omit for the full working tree. '
        'Use the full check for moves/deletions or changed link targets.'))
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[2]
    try:
        sources = selected_markdown(root, args.files) if args.files else None
        errors = check(root, sources)
    except (OSError, subprocess.CalledProcessError, UnicodeError, ValueError) as error:
        print(f'Markdown link check failed: {error}', file=sys.stderr)
        return 2
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    if sources is None:
        print('All working-tree Markdown local link targets exist.')
    else:
        print(f'Local link targets exist in {len(sources)} selected Markdown file(s); '
              'other sources were not checked.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
