"""Deterministic transport regressions; no application or timing speed threshold."""

import select
import socket
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


if __name__ == '__main__':
    unittest.main()
