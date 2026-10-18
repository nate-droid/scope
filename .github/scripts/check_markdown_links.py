#!/usr/bin/env python3
"""Check inline local Markdown links in tracked Markdown (not anchors)."""

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
        ['git', 'ls-files', '-z', '--', '*.md', '*.markdown',
         ':!:target/**', ':!:**/target/**'],
        cwd=root, check=True, stdout=subprocess.PIPE,
    )
    return [Path(name) for name in result.stdout.decode('utf-8').split('\0') if name]


def check(root):
    errors = []
    for source in tracked_markdown(root):
        for number, target in local_links((root / source).read_text(encoding='utf-8')):
            if not (root / source.parent / target).exists():
                errors.append(f'{source.as_posix()}:{number}: missing target: {target}')
    return errors


def main():
    root = Path(__file__).resolve().parents[2]
    try:
        errors = check(root)
    except (OSError, subprocess.CalledProcessError, UnicodeError, ValueError) as error:
        print(f'Markdown link check failed: {error}', file=sys.stderr)
        return 2
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print('All tracked Markdown local link targets exist.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
