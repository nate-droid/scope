#!/usr/bin/env python3
"""End-user workbench checks using the IDE's actual-terminal driver."""
import argparse
import importlib.util
from pathlib import Path
import signal
import sys
import unittest

ROOT = Path(__file__).resolve().parents[3]
# Do not leave import caches outside the new workbench tests directory.
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    'scope_ide_pty', ROOT / 'crates/scope-ide/tests/pty_shell.py')
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)
harness.BINARY = ROOT / 'target/debug/scope'


class WorkbenchTests(unittest.TestCase):
    def setUp(self):
        self.shell = harness.Shell()
        self.addCleanup(self.shell.close)

    def submit(self, command, expected):
        output = self.shell.send(command.encode('utf-8') + b'\r')
        self.assertIn(expected, output, repr(output))
        return output

    def test_literal_editor_run_repeat_edit_and_session_preservation(self):
        self.submit(':scope session', 'Active language: none')
        self.submit(':scope scratch new script', 'created (script)')
        self.submit(':scope edit --commands', 'Editing draft')
        self.shell.send('let answer = 40;\n'.encode())
        self.shell.send('if false { missing } else { print("雪😀") };\n'.encode())
        self.shell.send(b'answer + 2\n')
        self.assertIn('Returned to REPL', self.shell.send(b':done\n'))
        first = self.submit(':scope script run', 'final: 42')
        second = self.submit(':scope script run', 'final: 42')
        for output in (first, second):
            self.assertIn('print: "雪😀"', output)
            self.assertIn('status: interpreter success', output)
        def provenance(output):
            return next(line for line in output.splitlines() if line.startswith('script: '))
        self.assertEqual(provenance(first), provenance(second))
        # Change the original binding in place, then execute the same buffer.
        self.submit(':scope edit --commands', 'Editing draft')
        self.shell.send(b':at 1 14\n')
        self.shell.send(b':delete 2\n')
        self.shell.send(b':insert 41\n')
        self.assertIn('Returned to REPL', self.shell.send(b':done\n'))
        edited = self.submit(':scope script run', 'final: 43')
        self.assertNotEqual(provenance(first), provenance(edited))
        self.submit(':scope session', 'Active language: none')
        self.submit(':scope scratch new script', 'created (script)')
        self.submit(':scope edit --commands', 'Editing draft')
        self.shell.send(b'answer\n')
        self.shell.send(b':done\n')
        failed = self.submit(':scope script run', 'unknown binding')
        self.assertIn('status: interpreter failure', failed)
        self.submit(':scope session', 'Active language: none')
        # The interactive shell may persist its own history, but never source files.
        files = [p.relative_to(self.shell.workspace.name).as_posix()
                 for p in Path(self.shell.workspace.name).rglob('*') if p.is_file()]
        self.assertTrue(all(p.startswith('scope/') and 'history' in p for p in files), files)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, default=harness.BINARY)
    args, rest = parser.parse_known_args()
    harness.BINARY = args.binary.resolve()
    def timeout(signum, frame):
        raise TimeoutError('workbench PTY suite exceeded 90 seconds')
    signal.signal(signal.SIGALRM, timeout)
    signal.alarm(90)
    unittest.main(argv=[__file__] + rest)
