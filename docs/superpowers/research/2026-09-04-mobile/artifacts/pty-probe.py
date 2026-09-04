import os, pty, sys, time, select, re, signal
COLS, ROWS = int(sys.argv[1]), int(sys.argv[2])
sock = sys.argv[3]
env = dict(os.environ, TERM='xterm-256color', COLUMNS=str(COLS), LINES=str(ROWS))
env.pop('TMUX', None); env.pop('TMUX_PANE', None)
pid, fd = pty.fork()
if pid == 0:
    import fcntl, termios, struct
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, COLS, 0, 0))
    os.execvpe('tmux', ['tmux', '-L', sock, '-f', 'conf', 'new-session', '-s', 'tests'], env)
import fcntl, termios, struct
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, COLS, 0, 0))
out = b''
def drain(t):
    global out
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try: out += os.read(fd, 65536)
            except OSError: return
drain(4)
os.write(fd, b'cd .claude/\r'); drain(3)
os.write(fd, b'cd ski\t'); drain(3)
open('bytes.bin', 'wb').write(out)
os.system(f'tmux -L {sock} kill-server 2>/dev/null')
drain(1)
