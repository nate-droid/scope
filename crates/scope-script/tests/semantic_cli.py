"""Process-level DX1 checks. Build with cargo build -p scope-script --example semantic first."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
BINARY = TARGET / "debug" / "examples" / "semantic"


class SemanticCli(unittest.TestCase):
    def run_source(self, source, authorized=True):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "experiment.scope"
            data = source.encode() if isinstance(source, str) else source
            path.write_bytes(data)
            result = self.invoke(path, authorized)
            self.assertEqual(path.read_bytes(), data)
            self.assertEqual(list(Path(directory).iterdir()), [path])
            return result, str(path)

    def invoke(self, path, authorized=True):
        args = [str(BINARY)]
        if authorized:
            args.append("--allow-read")
        return subprocess.run(args + [str(path)], capture_output=True, text=True,
                              stdin=subprocess.DEVNULL, timeout=10)

    def test_authorized_repeat(self):
        source = (ROOT / "crates/scope-script/examples/semantic.scope").read_text()
        first, _ = self.run_source(source)
        second, _ = self.run_source(source)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(first.stdout, second.stdout)
        self.assertIn('String("answer")', first.stdout)
        self.assertEqual(first.stderr, "")

    def test_denial_keeps_prints_and_source_identity(self):
        result, path = self.run_source(
            'print("before");\nscope.invoke("semantic", '
            '"language.example.declarations.describe_selection", {})', False)
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, 'String("before")\n')
        self.assertIn(path + ":2:1:", result.stderr)
        self.assertIn("denied by policy", result.stderr)

    def test_structured_rejection_is_success(self):
        result, _ = self.run_source(
            'let operation = scope.operations()[0];\n'
            'let result = scope.invoke(operation.provider, operation.action, {unexpected: true});\n'
            'print(result.status);')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'String("invalid_arguments")\n')
        self.assertEqual(result.stderr, "")

    def test_parse_and_runtime_failures(self):
        result, path = self.run_source('print(1); let x = [')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "")
        self.assertIn(path + ":1:", result.stderr)
        self.assertIn("syntax error", result.stderr)
        result, path = self.run_source('print("é");\nmissing')
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, 'String("é")\n')
        self.assertIn(path + ":2:1:", result.stderr)
        self.assertIn("runtime error", result.stderr)

    def test_input_errors_identify_path(self):
        for source, message in [(b"\xff", "not UTF-8"), (b" " * 65537, "64 KiB")]:
            result, path = self.run_source(source)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(result.stdout, "")
            self.assertIn(path, result.stderr)
            self.assertIn(message, result.stderr)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "missing.scope"
            result = self.invoke(path)
            self.assertEqual(result.returncode, 1)
            self.assertIn(str(path), result.stderr)
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_extra_arguments(self):
        result = subprocess.run([str(BINARY), "--allow-read", "a", "b"],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn("usage:", result.stderr)


if __name__ == "__main__":
    unittest.main()
