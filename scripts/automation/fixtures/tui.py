#!/usr/bin/env python3
"""A deterministic interactive TUI for automation checks.

It takes the alternate screen, enables bracketed paste, and shows a prompt.
Pasted text (including newlines) goes into the draft; Enter outside a paste
submits the draft and prints `SUBMITTED <n>` with every submitted line.
`q` on an empty draft quits with status 0. Ctrl+C quits with status 130.
"""
import os
import sys
import termios
import tty

def main():
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    out = sys.stdout
    draft = ""
    submissions = []
    pasting = False
    pending = b""

    def draw():
        out.write("\x1b[H\x1b[2J")
        out.write("TUI FIXTURE  (paste text, Enter submits, q quits)\r\n")
        for i, lines in enumerate(submissions, 1):
            out.write(f"SUBMITTED {i}\r\n")
            for line in lines.split("\n"):
                out.write(f"  | {line}\r\n")
        out.write("PROMPT> " + draft.replace("\n", "\r\n        ") + "")
        out.flush()

    out.write("\x1b[?1049h\x1b[?2004h\x1b]0;tui-fixture\x07")
    tty.setraw(fd)
    status = 0
    try:
        draw()
        while True:
            chunk = os.read(fd, 4096)
            if not chunk:
                break
            pending += chunk
            while pending:
                if pending.startswith(b"\x1b[200~"):
                    pasting = True
                    pending = pending[6:]
                    continue
                if pending.startswith(b"\x1b[201~"):
                    pasting = False
                    pending = pending[6:]
                    continue
                if pending.startswith(b"\x1b") and len(pending) < 6 and b"\x1b[200~".startswith(pending[:6]):
                    break
                # Decode one UTF-8 character.
                for size in (1, 2, 3, 4):
                    try:
                        ch = pending[:size].decode("utf-8")
                        break
                    except UnicodeDecodeError:
                        ch = None
                if ch is None:
                    if len(pending) < 4:
                        break
                    pending = pending[1:]
                    continue
                pending = pending[len(ch.encode("utf-8")):]
                if pasting:
                    draft += "\n" if ch in "\r\n" else ch
                elif ch == "\x03":
                    status = 130
                    return status
                elif ch == "\r":
                    submissions.append(draft)
                    draft = ""
                elif ch == "\x7f":
                    draft = draft[:-1]
                elif ch == "q" and not draft:
                    return status
                elif ch >= " ":
                    draft += ch
            draw()
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)
        out.write("\x1b[?2004l\x1b[?1049l")
        out.flush()
    return status

if __name__ == "__main__":
    sys.exit(main())
