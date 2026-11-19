"""Deterministic transport regressions; no application or timing speed threshold."""


import os
import pickle
from pathlib import Path
import select
import shutil
import signal
import socket
import sys
import tempfile
import threading
import time
import unittest

from pty_editable_workspace import Workspace


class CommandSynchronizationTests(unittest.TestCase):
    def workspace(self):
        reader, peer = socket.socketpair()
        self.addCleanup(reader.close)
        self.addCleanup(peer.close)
        workspace = Workspace.__new__(Workspace)
        workspace.fd = reader.fileno()
        workspace.raw = bytearray()
        workspace.pending = b''
        workspace.deadline = time.monotonic() + 5
        return workspace, peer

    def test_fragmented_delayed_command_and_cursor_query(self):
        workspace, peer = self.workspace()
        release = threading.Event()
        result = []
        failures = []

        def command():
            try:
                result.append(workspace.command(':scope show'))
            except BaseException as error:
                failures.append(error)
            finally:
                release.set()

        worker = threading.Thread(target=command)
        worker.start()
        self.addCleanup(worker.join, 6)
        peer.settimeout(2)
        self.assertEqual(peer.recv(65536), b':scope show\r')
        # A redraw prompt alone is not completion, nor is the submitted echo.
        peer.sendall(b'\x1b[2Kscope> ')
        self.assertFalse(release.wait(.05))
        peer.sendall(b':scope show\r\n')
        self.assertFalse(release.wait(.4))  # longer than the old quiet interval
        # Deliberately split cursor query, CSI, Unicode and the final prompt.
        for fragment in (b'\x1b[', b'6', b'n'):
            peer.sendall(fragment)
        self.assertEqual(peer.recv(65536), b'\x1b[1;1R')
        wire = '\x1b[32m雪😀\x1b[0m\r\nscope> '.encode()
        for byte in wire[:-1]:
            peer.sendall(bytes([byte]))
            time.sleep(.001)
        self.assertFalse(release.wait(.05))
        peer.sendall(wire[-1:])
        self.assertTrue(release.wait(2), 'command did not recognize final prompt')
        worker.join(2)
        self.assertFalse(worker.is_alive())
        self.assertEqual(failures, [])
        self.assertIn('雪😀\nscope> ', result[0])
        self.assertIn(wire, workspace.raw)

    def test_send_until_fragmented_delayed_marker_and_cursor_query(self):
        workspace, peer = self.workspace()
        release = threading.Event()
        result = []
        failures = []
        needle = '雪😀 ready'

        def send_until():
            try:
                result.append(workspace.send_until(b'x', needle))
            except BaseException as error:
                failures.append(error)
            finally:
                release.set()

        worker = threading.Thread(target=send_until)
        worker.start()
        self.addCleanup(worker.join, 6)
        peer.settimeout(2)
        self.assertEqual(peer.recv(65536), b'x')
        peer.sendall(b'early unrelated text\r\n')
        self.assertFalse(release.wait(.4))  # longer than the old quiet interval
        for fragment in (b'\x1b[', b'6', b'n'):
            peer.sendall(fragment)
        self.assertEqual(peer.recv(65536), b'\x1b[1;1R')
        wire = ('\x1b[32m' + needle[:-1]).encode()
        for byte in wire:
            peer.sendall(bytes([byte]))
        self.assertFalse(release.wait(.05))
        # The returned read must retain text following the marker as well.
        peer.sendall(b'y\x1b[0m\r\nafter marker')
        self.assertTrue(release.wait(2), 'send_until did not recognize marker')
        worker.join(2)
        self.assertFalse(worker.is_alive())
        self.assertEqual(failures, [])
        self.assertEqual(result, ['early unrelated text\n' + needle + '\nafter marker'])

    def test_send_until_eof_has_needle_and_output(self):
        workspace, peer = self.workspace()
        peer.sendall(b'unfinished output')
        peer.shutdown(socket.SHUT_WR)
        with self.assertRaises(AssertionError) as raised:
            workspace.send_until(b'x', 'missing marker')
        self.assertIn('missing marker', str(raised.exception))
        self.assertIn('unfinished output', str(raised.exception))
        self.assertEqual(peer.recv(65536), b'x')

    def test_send_until_bounded_by_timeout_and_scenario_deadline(self):
        for bound in ('timeout', 'scenario'):
            with self.subTest(bound=bound):
                workspace, peer = self.workspace()
                if bound == 'scenario':
                    workspace.deadline = time.monotonic() + .1
                release = threading.Event()
                failures = []

                def send_until():
                    try:
                        workspace.send_until(b'x', 'missing marker',
                                             timeout=.1 if bound == 'timeout' else 10)
                    except BaseException as error:
                        failures.append(error)
                    finally:
                        release.set()

                peer.sendall(b'unfinished output')
                worker = threading.Thread(target=send_until)
                worker.start()
                self.addCleanup(worker.join, 6)
                peer.settimeout(2)
                self.assertEqual(peer.recv(65536), b'x')
                self.assertTrue(release.wait(2), 'send_until did not honor bounded deadline')
                worker.join(2)
                self.assertFalse(worker.is_alive())
                self.assertEqual(len(failures), 1)
                self.assertIsInstance(failures[0], AssertionError)
                self.assertIn('missing marker', str(failures[0]))
                self.assertIn('unfinished output', str(failures[0]))

    def test_send_until_empty_needle_is_rejected_before_writing(self):
        workspace, peer = self.workspace()
        with self.assertRaises(ValueError):
            workspace.send_until(b'x', '')
        self.assertFalse(select.select([peer], [], [], 0)[0])

    def test_no_output_command(self):
        workspace, peer = self.workspace()
        peer.sendall(b'scope> :scope session\r\nscope> ')
        self.assertTrue(workspace.command(':scope session').endswith('scope> '))

    def test_missing_prompt_timeout_has_command_and_output(self):
        workspace, peer = self.workspace()
        workspace.deadline = time.monotonic() + .1
        peer.sendall(b'scope> :scope show\r\nunfinished')
        with self.assertRaisesRegex(AssertionError, 'post-submission prompt.*:scope show.*unfinished'):
            workspace.command(':scope show')

    def test_eof_is_not_completion(self):
        workspace, peer = self.workspace()
        peer.sendall(b'scope> :scope show\r\nunfinished')
        peer.shutdown(socket.SHUT_WR)
        with self.assertRaisesRegex(AssertionError, 'post-submission prompt'):
            workspace.command(':scope show')

    def test_unsupported_input_is_rejected_before_writing(self):
        workspace, peer = self.workspace()
        for command in (':scope show\n', '雪', '\x1b'):
            with self.assertRaises(ValueError):
                workspace.command(command)
        self.assertFalse(select.select([peer], [], [], 0)[0])


class WorkspaceLifecycleTests(unittest.TestCase):
    def fixture_binary(self, mode):
        directory = tempfile.TemporaryDirectory(prefix='scope-pty-fixture-')
        self.addCleanup(directory.cleanup)
        binary = Path(directory.name) / 'worker'
        binary.write_text(
            f'#!{sys.executable}\n'
            'import os, signal, sys, time\n'
            f'mode = {mode!r}\n'
            'if mode == "ignore-term":\n'
            '    signal.signal(signal.SIGTERM, signal.SIG_IGN)\n'
            'print("READY", os.getpid(), flush=True)\n'
            'if mode in ("success", "failure"):\n'
            '    os.read(0, 1)\n'
            '    sys.exit(0 if mode == "success" else 23)\n'
            'while True:\n'
            '    time.sleep(1)\n')
        binary.chmod(0o755)
        return binary

    def fixture(self, mode):
        workspace = Workspace(self.fixture_binary(mode))
        self.addCleanup(workspace.close)
        output = workspace.expect('READY')
        worker = int(output.split('READY ')[1].split()[0])
        return workspace, worker

    def assert_clean(self, workspace, worker):
        descriptors = (workspace.fd, workspace.slave, workspace.restore_read)
        directory = workspace.temp.name
        workspace.close()
        workspace.close()
        self.assertIsNotNone(workspace.status)
        self.assertFalse(Path(directory).exists())
        for descriptor in descriptors:
            with self.assertRaises(OSError):
                os.fstat(descriptor)
        with self.assertRaises(ChildProcessError):
            os.waitpid(workspace.pid, os.WNOHANG)
        with self.assertRaises(ProcessLookupError):
            os.kill(worker, 0)
        self.assertFalse(os.WIFSIGNALED(workspace.status),
                         'supervisor must survive group termination and reap worker')

    def test_success_and_nonzero_exit_are_reaped(self):
        for mode, expected in (('success', 0), ('failure', 23)):
            with self.subTest(mode=mode):
                workspace, worker = self.fixture(mode)
                workspace.send(b'x\r')
                self.assertEqual(workspace.wait_exit(), expected)
                self.assertEqual(workspace.wait_exit(), expected)
                self.assert_clean(workspace, worker)

    def test_assertion_failure_cleanup_reaps_worker(self):
        workspace, worker = self.fixture('wait')
        try:
            with self.assertRaisesRegex(AssertionError, 'missing marker'):
                workspace.send_until(b'x', 'missing marker', timeout=.05)
        finally:
            self.assert_clean(workspace, worker)

    def test_timeout_cleanup_escalates_term_resistant_worker(self):
        workspace, worker = self.fixture('ignore-term')
        with self.assertRaisesRegex(AssertionError, 'bounded timeout'):
            workspace.wait_exit(timeout=.05)
        start = time.monotonic()
        self.assert_clean(workspace, worker)
        self.assertLess(time.monotonic() - start, 3,
                        'TERM-resistant worker exceeded supervisor cleanup bound')
        self.assertEqual(os.waitstatus_to_exitcode(workspace.status), 1)

    def test_abrupt_driver_death_reaps_term_resistant_worker(self):
        reader, sender = socket.socketpair()
        self.addCleanup(reader.close)
        self.addCleanup(sender.close)
        binary = self.fixture_binary('ignore-term')
        driver = os.fork()
        if driver == 0:
            try:
                reader.close()
                workspace = Workspace(binary)
                output = workspace.expect('READY')
                worker = int(output.split('READY ')[1].split()[0])
                socket.send_fds(sender, [pickle.dumps(
                    (workspace.pid, worker, workspace.temp.name))],
                    [workspace.fd, workspace.slave, workspace.restore_read])
                while True:
                    signal.pause()
            finally:
                os._exit(127)

        sender.close()
        try:
            reader.settimeout(5)
            data, descriptors, flags, address = socket.recv_fds(reader, 65536, 3)
            for descriptor in descriptors:
                self.addCleanup(os.close, descriptor)
            self.assertEqual(len(descriptors), 3)
            supervisor, worker, directory = pickle.loads(data)
            self.addCleanup(shutil.rmtree, directory, True)
            os.kill(driver, signal.SIGKILL)
            _, status = os.waitpid(driver, 0)
            self.assertEqual(os.waitstatus_to_exitcode(status), -signal.SIGKILL)
            driver = None
            reaped = False
            try:
                # This report is emitted only after the supervisor waitpid reaps
                # the worker. Keep the PTY and report pipe alive independently
                # of the driver, so terminal hangup cannot make this test pass.
                restore_read = descriptors[2]
                self.assertTrue(select.select([restore_read], [], [], 3)[0],
                                'orphan supervisor did not reap and report')
                self.assertIsInstance(pickle.loads(os.read(restore_read, 65536)), list)
                with self.assertRaises(ProcessLookupError):
                    os.kill(worker, 0)
                reaped = True
            finally:
                if not reaped:
                    try:
                        os.killpg(supervisor, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
        finally:
            if driver is not None:
                os.kill(driver, signal.SIGKILL)
                os.waitpid(driver, 0)

    def test_exec_failure_is_reaped_and_close_is_idempotent(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Workspace(Path(directory) / 'missing')
            self.addCleanup(workspace.close)
            self.assertEqual(workspace.wait_exit(), 127)
            workspace.close()
            workspace.close()
            with self.assertRaises(ChildProcessError):
                os.waitpid(workspace.pid, os.WNOHANG)


if __name__ == '__main__':
    unittest.main()
