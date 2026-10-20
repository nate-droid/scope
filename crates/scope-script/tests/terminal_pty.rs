//! Unix PTY integration tests. Requires `python3` on PATH (Python stdlib only).
//! Run with: cargo test -p scope-script --test terminal_pty
//!
//! The slave stays open in the driver so termios can be compared after exit.
//! File-mode errors cover a practical nonzero exit; killing a process or hanging
//! up its PTY cannot meaningfully verify the reader's RAII error cleanup.
#![cfg(unix)]

use std::process::Command;

fn pty_test(scenario: &str) {
    let output = Command::new("python3")
        .args(["-c", DRIVER, env!("CARGO_BIN_EXE_scope-script"), scenario])
        .output()
        .expect("PTY tests require python3 on PATH with the standard-library pty module");
    assert!(
        output.status.success(),
        "PTY scenario {scenario} failed ({}):\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn editing_at_arbitrary_positions() {
    pty_test("editing");
}

#[test]
fn history_restores_draft_and_cursor() {
    pty_test("history");
}

#[test]
fn ctrl_c_discards_partial_input_without_evaluation() {
    pty_test("cancel");
}

#[test]
fn ctrl_d_deletes_in_nonempty_line_and_exits_on_empty_line() {
    pty_test("eof");
}

#[test]
fn evaluation_errors_recover_to_prompt() {
    pty_test("recovery");
}

#[test]
fn error_exit_preserves_termios() {
    pty_test("error_exit");
}

#[test]
fn tab_completion_uses_current_environment() {
    pty_test("completion");
}

const DRIVER: &str = r#"
import errno
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

# A hard watchdog also bounds bugs in the driver itself. All child cleanup runs
# through finally, including timeout failures; no sleeps or human timing needed.
def timeout(signum, frame):
    raise TimeoutError('PTY scenario exceeded 20 seconds')
signal.signal(signal.SIGALRM, timeout)
signal.alarm(20)
master, slave = pty.openpty()
child = None
transcript = bytearray()
pending = bytearray()
try:
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 120, 0, 0))
    before = termios.tcgetattr(slave)
    os.set_blocking(master, False)

    def read_once(deadline):
        remaining = deadline - time.monotonic()
        assert remaining > 0, 'timed out waiting for terminal output'
        ready, _, _ = select.select([master], [], [], remaining)
        assert ready, 'timed out waiting for terminal output'
        try:
            data = os.read(master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                raise AssertionError('PTY closed before expected output') from error
            raise
        assert data, 'PTY reached EOF before expected output'
        transcript.extend(data)
        pending.extend(data)

    def expect(token):
        deadline = time.monotonic() + 4
        while token not in pending:
            # Keeping the slave open prevents EOF, so detect early exits too.
            if child.poll() is not None:
                ready, _, _ = select.select([master], [], [], 0)
                assert ready, 'child exited before ' + repr(token)
            read_once(deadline)
        end = pending.index(token) + len(token)
        result = bytes(pending[:end])
        del pending[:end]
        return result

    def send(data):
        deadline = time.monotonic() + 4
        while data:
            remaining = deadline - time.monotonic()
            assert remaining > 0, 'timed out writing PTY input'
            _, ready, _ = select.select([], [master], [], remaining)
            assert ready, 'timed out writing PTY input'
            try:
                count = os.write(master, data)
                data = data[count:]
            except BlockingIOError:
                pass

    def result(keys, value):
        send(keys + b'\r')
        expect(b'=> Integer(' + str(value).encode() + b')')
        expect(b'scope> ')

    def finish(keys=b':quit\r', status=0):
        if keys:
            send(keys)
        assert child.wait(timeout=4) == status, 'unexpected child exit status'
        # Drain all final bytes without relying on EOF (we retain the slave).
        while select.select([master], [], [], 0)[0]:
            data = os.read(master, 65536)
            if not data:
                break
            transcript.extend(data)
        assert termios.tcgetattr(slave) == before, 'terminal attributes were not restored'

    scenario = sys.argv[2]
    with tempfile.TemporaryDirectory(prefix='scope-script-pty-') as directory:
        args = [sys.argv[1]]
        if scenario == 'error_exit':
            path = os.path.join(directory, 'error.scope')
            with open(path, 'w') as script:
                script.write('1 / 0\n')
            args.append(path)
        env = dict(os.environ, TERM='xterm-256color', NO_COLOR='1')
        # A new session prevents accidental signals to the cargo test process.
        # All three standard streams refer to the same real terminal.
        child = subprocess.Popen(args, stdin=slave, stdout=slave, stderr=slave,
                                 start_new_session=True, env=env, close_fds=True)
        if scenario == 'error_exit':
            expect(b'runtime error:')
            finish(keys=None, status=1)
        else:
            expect(b'scope> ')
            if scenario == 'editing':
                # 13 -> 123 -> 124; Home/Right/Delete and End/Backspace
                # then produce 24 + 5. Each edit affects the evaluated value.
                result(b'13\x1b[D2\x1b[C\x7f4\x1b[H\x1b[C\x1b[3~'
                       b'\x1b[H\x1b[3~2\x1b[F9\x7f + 5', 29)
                result(b'23\x01' + b'1+\x05+4', 28)
                finish()
            elif scenario == 'completion':
                result(b'le\t answer = 42', 42)
                result(b'ans\t', 42)
                send(b'pri\t(answer)\r')
                expect(b'Integer(42)')
                expect(b'scope> ')
                # Replace the entire token while preserving the suffix expression.
                result(b'answrong + 1\x01\x1b[C\x1b[C\x1b[C\t\x05', 43)
                send(b'scope.\t')
                expect(b'scope.invoke')
                expect(b'scope.operations')
                expect(b'scope> scope.')
                send(b'\x03')
                expect(b'\r\n')
                expect(b'scope> ')
                send(b'unknown\t\r')
                expect(b'unknown binding')
                expect(b'scope> ')
                result(b'2', 2)
                finish()
            elif scenario == 'history':
                result(b'11', 11)
                result(b'22', 22)
                # Save draft "13" with cursor between digits. Mutate a recalled
                # entry, walk to oldest/newest, and restore the original draft.
                result(b'13\x1b[D\x1b[A9\x1b[A\x1b[A\x1b[B\x1b[B2', 123)
                # Recall edits must not have mutated the stored "22".
                result(b'\x1b[A\x1b[A', 22)
                finish()
            elif scenario == 'cancel':
                send(b'print(918273)')
                # Wait for the full draft redraw before sending Ctrl-C: otherwise
                # canonical mode could swallow it before raw mode is active.
                expect(b'scope> print(918273)')
                send(b'\x03')
                expect(b'\r\n')
                expect(b'scope> ')
                result(b'6 * 7', 42)
                finish()
                assert b'Integer(918273)' not in transcript, 'cancelled input evaluated'
                assert transcript.count(b'=> Integer(') == 1, 'unexpected partial evaluation'
            elif scenario == 'eof':
                # Ctrl-D deletes under the cursor, but at end of nonempty input
                # is a no-op, not EOF. On the next empty prompt it exits.
                result(b'123\x01\x1b[C\x04\x05\x04', 13)
                finish(b'\x04')
            elif scenario == 'recovery':
                send(b'1 / 0\r')
                expect(b'runtime error:')
                expect(b'scope> ')
                send(b'let x = [\r')
                expect(b'syntax error:')
                expect(b'scope> ')
                result(b'40 + 2', 42)
                finish()
            else:
                raise AssertionError('unknown scenario: ' + scenario)
except BaseException:
    print('PTY transcript: ' + repr(bytes(transcript)), file=sys.stderr)
    raise
finally:
    signal.alarm(0)
    if child is not None:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=4)
    os.close(master)
    os.close(slave)
"#;
