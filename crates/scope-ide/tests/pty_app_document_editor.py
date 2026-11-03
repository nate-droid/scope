#!/usr/bin/env python3
"""Real ordinary-launcher App edit/publish/confirm path; no shell handoff."""
import os
from pathlib import Path
import shlex
import sys
import tempfile

from pty_editable_workspace import Workspace


def main():
    with tempfile.TemporaryDirectory(prefix="scope-app-editor-pty-") as root:
        source = Path(root) / "first.inspectone"
        source.write_text("saved first")
        wrapper = Path(root) / "launcher"
        wrapper.write_text(
            "#!/bin/sh\nexport SCOPE_APP_EDITOR_PTY_CHILD=1\n"
            f"cd {shlex.quote(root)}\n"
            f"exec {shlex.quote(sys.argv[1])} --exact app_document_launcher_child --ignored --nocapture\n"
        )
        wrapper.chmod(0o700)
        workspace = Workspace(wrapper)
        try:
            def send(keys):
                os.write(workspace.fd, keys)
                return workspace.read(timeout=0.5)

            workspace.expect("Read-onlyinspector")

            def action(label):
                workspace.expect("Actions:", send(b"\x1bOP"))
                selected = send(label.encode())
                return selected + send(b"\r")

            opened = action("open-app")
            workspace.expect("Open document:", opened)
            send(b"first.inspectone\r")
            # The source has eleven scalars; delete at the initial cursor, then paste.
            send(b"\x1b[3~" * 11)
            send(b"\x1b[200~" + "unsaved λ".encode() + b"\x1b[201~")
            blocked = action("evaluate")
            assert "disabled" in blocked or "draft" in blocked, repr(blocked)
            send(b"\x1b")  # close the picker, not the editor
            send(b"\x13")  # publish to App, never Save
            reviewed = action("evaluate")
            workspace.expect("fixture.one.alpha", reviewed)
            confirmation = send(b"\r")
            workspace.expect("Inspectcurrentdocument?", confirmation)
            assert "unsaved" in confirmation, repr(confirmation)
            assert "CURRENT" not in confirmation, repr(confirmation)
            result = send(b"\r")
            workspace.expect("snapshot-marker", result)
            send(b"e")  # full retained native evidence
            send(b"\x1b")
            workspace.expect("Unsaved", send(b"\x11"))
            send(b"\x11")
            assert workspace.wait_exit() == 0, bytes(workspace.raw)
            assert source.read_text() == "saved first", "publication must not save to disk"
            assert b"\x1b[?1049l" in workspace.raw, "terminal was not restored"
            print("App open/edit/publish/explicit confirmation passed; exact source asserted by child")
        finally:
            workspace.close()


if __name__ == "__main__":
    main()
