"""Unix smoke test: build example, then python3 completion_pty.py PATH_TO_EXAMPLE."""
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time


def timeout(*_):
    raise TimeoutError("completion PTY exceeded 20 seconds")


def run_mode(executable, arguments):
    signal.alarm(20)
    master, slave = pty.openpty()
    child = None
    try:
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        before = termios.tcgetattr(slave)
        def own_terminal():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        child = subprocess.Popen(
            [executable, *arguments], stdin=slave, stdout=slave, stderr=slave,
            preexec_fn=own_terminal,
        )
        pending = bytearray()
        transcript = bytearray()

        def expect(text):
            deadline = time.monotonic() + 5
            while text not in pending:
                ready, _, _ = select.select([master], [], [], max(0, deadline - time.monotonic()))
                assert ready, (text, pending)
                chunk = os.read(master, 65536)
                assert chunk, ("terminal closed while expecting", text)
                pending.extend(chunk)
                transcript.extend(chunk)
            end = pending.index(text) + len(text)
            del pending[:end]

        expect(b"commands> ")
        os.write(master, b"he\t")
        expect(b"commands> hel")
        os.write(master, b"\t")
        expect(b"help -- show commands\r\nhello -- greeting\\n\\u{1b}[2J\\u{4e2d}\r\n")
        expect(b"commands> hel")
        os.write(master, b"p\t\r")
        expect(b'submitted: "help"')
        expect(b"commands> ")
        os.write(master, b"\x1b[A")
        expect(b"commands> help")
        os.write(master, b"\x03")
        expect(b"commands> ")
        # Bracketed paste remains insertion-only, even after displaying descriptions.
        os.write(master, b"\x1b[200~ha\x1b[201~\t\r")
        expect(b'submitted: "halt"')
        expect(b"commands> ")
        # Display at a narrow width must not wrap metadata onto an extra line.
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 20, 0, 0))
        child.send_signal(signal.SIGWINCH)
        time.sleep(0.1)
        os.write(master, b"hel\t")
        expect(b"help -- show comman\r\nhello -- greeting\\n\r\n")
        expect(b"commands>hel")
        # Move into the line, display again, and confirm cursor restoration by editing.
        os.write(master, b" tail\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D\t")
        expect(b"help -- show comman\r\nhello -- greeting\\n\r\n")
        expect(b"commands>hel tail")
        os.write(master, b"X\r")
        expect(b'submitted: "helX tail"')
        expect(b"commands>")
        # A paste is insertion-only: embedded line breaks and controls must not
        # submit, complete, interrupt, or escape into terminal commands.
        paste = (
            'alpha\n\tbeta\n\n中 café🙂\r\ngamma\rdelta '
            '"quoted"\\path\x1b[31mRED\x1b[0m'
            '\x1b]0;not-a-title\x07\x1bPdiscarded\x1b\\'
            '\x00\x03\x04\x07\x08\x7f END'
        )
        expected = (
            b'"alpha\\n\\tbeta\\n\\n' + '中 café🙂'.encode('utf-8')
            + b'\\ngamma\\ndelta \\"quoted\\"\\\\pathRED END"'
            if arguments else
            b'"alpha beta  ' + '中 café🙂'.encode('utf-8')
            + b' gamma delta \\"quoted\\"\\\\pathRED END"'
        )
        submissions_before = transcript.count(b"submitted:")
        os.write(master, b"\x1b[200~" + paste.encode('utf-8') + b"\x1b[201~")
        # Drain redraws for a bounded quiet interval before sending the only Enter.
        deadline = time.monotonic() + 0.4
        while True:
            ready, _, _ = select.select([master], [], [], max(0, deadline - time.monotonic()))
            if not ready:
                break
            chunk = os.read(master, 65536)
            assert chunk, "terminal closed before Enter"
            pending.extend(chunk)
            transcript.extend(chunk)
            assert b"submitted:" not in pending, pending
            assert time.monotonic() < deadline + 1, "paste output did not settle"
        assert b"submitted:" not in pending, pending
        assert child.poll() is None, "paste controls terminated the example"
        os.write(master, b"\r")
        expect(b"submitted: " + expected + b"\r\n")
        expect(b"commands>")
        os.write(master, b"\x04")
        expect(b"\x1b[?2004l")
        assert child.wait(timeout=5) == 0
        assert transcript.count(b"submitted:") == submissions_before + 1, (
            "expected exactly one paste submission", transcript
        )
        assert termios.tcgetattr(master) == before, "terminal settings not restored"
    finally:
        if child is not None and child.poll() is None:
            child.kill()
            child.wait(timeout=2)
        os.close(master)
        os.close(slave)
        signal.alarm(0)


def run_redirected(executable, arguments):
    # Policy must not affect fallback framing, tabs/escapes, CRLF, or final CR.
    data = 'α\tfirst\r\n\n\x1b[31msecond\nlast\r'.encode('utf-8')
    expected = (
        'submitted: "α\\tfirst"\nsubmitted: ""\n'.encode('utf-8')
        + b'submitted: "\\u{1b}[31msecond"\nsubmitted: "last\\r"\n'
    )
    result = subprocess.run(
        [executable, *arguments], input=data, capture_output=True, timeout=5,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout == expected, result.stdout


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, timeout)
    for arguments in ([], ["--normalized-layout"]):
        run_redirected(sys.argv[1], arguments)
        run_mode(sys.argv[1], arguments)
        print("completion PTY passed:", " ".join(arguments) or "default")
