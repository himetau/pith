#!/usr/bin/env python3
# A pty for the terminal pane without a native node module. stdin goes to the shell, the shell's output
# comes out on stdout, and "cols rows" lines on fd 3 resize it. usage: shell-pty.py <cwd> [shell]
import fcntl, os, pty, select, signal, struct, sys, termios

cwd = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~")
shell = sys.argv[2] if len(sys.argv) > 2 else os.environ.get("SHELL", "/bin/sh")

pid, master = pty.fork()
if pid == 0:
    try:
        os.chdir(cwd)
    except OSError:
        pass
    os.execvp(shell, [shell, "-l"])

def resize(cols, rows):
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

ctl = 3
try:
    os.fstat(ctl)
except OSError:
    ctl = None

signal.signal(signal.SIGTERM, lambda *_: (os.kill(pid, signal.SIGHUP), sys.exit(0)))
fds = [master, 0] + ([ctl] if ctl is not None else [])
ctl_buf = b""
while True:
    try:
        ready, _, _ = select.select(fds, [], [])
    except InterruptedError:
        continue
    if master in ready:
        try:
            data = os.read(master, 65536)
        except OSError:
            data = b""
        if not data:
            break
        os.write(1, data)
    if 0 in ready:
        data = os.read(0, 65536)
        if not data:
            # the app went away, take the shell with it
            os.kill(pid, signal.SIGHUP)
            break
        os.write(master, data)
    if ctl is not None and ctl in ready:
        chunk = os.read(ctl, 4096)
        if not chunk:
            fds.remove(ctl)
            ctl = None
            continue
        ctl_buf += chunk
        while b"\n" in ctl_buf:
            line, ctl_buf = ctl_buf.split(b"\n", 1)
            parts = line.split()
            if len(parts) == 2:
                resize(int(parts[0]), int(parts[1]))

_, status = os.waitpid(pid, 0)
sys.exit(os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else 0)
