import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_markdown_links as checker


class MarkdownLinksTests(unittest.TestCase):
    def test_local_destinations(self):
        text = ('[file](../guide.md#section) ![image](images/a%20b.png)\n'
                '[title](guide.md "Read this") [spaces](<my guide.md>)\n'
                '[parens](guide(v2).md) [escaped](guide\\(v3\\).md)\n'
                '[query](guide.md?raw=1#part)')
        self.assertEqual(list(checker.local_links(text)), [
            (1, '../guide.md'), (1, 'images/a b.png'),
            (2, 'guide.md'), (2, 'my guide.md'),
            (3, 'guide(v2).md'), (3, 'guide(v3).md'), (4, 'guide.md'),
        ])

    def test_nonlocal_and_fragment_links(self):
        text = ('[web](https://example.com/a) [mail](mailto:a@example.com) '
                '[other](custom:thing) [relative](//example.com/a) '
                '[anchor](#section) [empty]()')
        self.assertEqual(list(checker.local_links(text)), [])

    def test_fenced_and_inline_code(self):
        text = ('````markdown\n[hidden](missing.md)\n```\n'
                '[still hidden](missing.md)\n````\n'
                '   ~~~\n[hidden](missing.md)\n   ~~~\n'
                '`[code](missing.md)` [real](present.md)\n')
        self.assertEqual(list(checker.local_links(text)), [(9, 'present.md')])

    def test_reports_source_line_and_checks_relative_decoded_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'docs').mkdir()
            (root / 'a b.txt').write_text('present', encoding='utf-8')
            (root / 'docs/readme.md').write_text(
                '[ok](../a%20b.txt#ignored)\n[bad](missing.md)\n'
                '[directory](../docs)\n', encoding='utf-8')
            with patch.object(checker, 'tracked_markdown', return_value=[Path('docs/readme.md')]):
                self.assertEqual(checker.check(root), [
                    'docs/readme.md:2: missing target: missing.md',
                ])

    def test_git_uses_nul_delimiters_and_excludes_target_directories(self):
        with patch.object(checker.subprocess, 'run', return_value=subprocess.CompletedProcess(
                [], 0, stdout=b'README.md\0docs/a b.md\0')) as run:
            self.assertEqual(checker.tracked_markdown(Path('.')), [
                Path('README.md'), Path('docs/a b.md')])
        self.assertEqual(run.call_args.args[0], [
            'git', 'ls-files', '-z', '--', '*.md', '*.markdown',
            ':!:target/**', ':!:**/target/**'])
        self.assertTrue(run.call_args.kwargs['check'])


if __name__ == '__main__':
    unittest.main()
