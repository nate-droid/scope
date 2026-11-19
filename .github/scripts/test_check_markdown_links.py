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

    def test_explicit_sources_skip_discovery_but_retain_missing_link_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'selected.md').write_text('[bad](missing.md)', encoding='utf-8')
            (root / 'unrelated.md').write_text('[bad](other.md)', encoding='utf-8')
            with patch.object(checker, 'tracked_markdown', side_effect=AssertionError('discovery')):
                sources = checker.selected_markdown(root, ['selected.md', 'selected.md'])
                self.assertEqual(sources, [Path('selected.md')])
                self.assertEqual(checker.check(root, sources), [
                    'selected.md:1: missing target: missing.md'])
                (root / 'missing.md').touch()
                self.assertEqual(checker.check(root, sources), [])

    def test_explicit_sources_reject_invalid_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'present.txt').touch()
            for name in ['missing.md', 'present.txt', '../outside.md', str(root / 'absolute.md')]:
                with self.subTest(name=name), self.assertRaises(ValueError):
                    checker.selected_markdown(root, [name])
            (root / 'escape.md').symlink_to(root.parent / 'outside.md')
            with self.assertRaises(ValueError):
                checker.selected_markdown(root, ['escape.md'])

    def test_cli_selection_and_exit_status(self):
        with patch.object(checker, 'selected_markdown', return_value=[Path('README.md')]) as select, \
                patch.object(checker, 'check', return_value=[]) as check, \
                patch('builtins.print') as output:
            self.assertEqual(checker.main(['README.md']), 0)
            self.assertEqual(select.call_args.args[1], ['README.md'])
            self.assertEqual(check.call_args.args[1], [Path('README.md')])
            self.assertIn('other sources were not checked', output.call_args.args[0])
        with patch.object(checker, 'check', return_value=['missing target']), patch('builtins.print'):
            self.assertEqual(checker.main(['README.md']), 1)
        with patch.object(checker, 'selected_markdown', side_effect=ValueError('invalid')), \
                patch('builtins.print'):
            self.assertEqual(checker.main(['missing.md']), 2)

    def test_cli_default_keeps_full_check(self):
        with patch.object(checker, 'check', return_value=[]) as check, patch('builtins.print'):
            self.assertEqual(checker.main([]), 0)
            self.assertIsNone(check.call_args.args[1])

    def test_git_uses_nul_delimiters_and_excludes_target_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'docs').mkdir()
            (root / 'README.md').touch()
            (root / 'docs/a b.md').touch()
            with patch.object(checker.subprocess, 'run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout=b'README.md\0docs/a b.md\0deleted.md\0README.md\0')) as run:
                self.assertEqual(checker.tracked_markdown(root), [
                    Path('README.md'), Path('docs/a b.md')])
        self.assertEqual(run.call_args.args[0], [
            'git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*.md', '*.markdown',
            ':!:target/**', ':!:**/target/**'])
        self.assertTrue(run.call_args.kwargs['check'])


if __name__ == '__main__':
    unittest.main()
