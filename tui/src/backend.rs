//! Works around ratatui-crossterm 0.1.2 misplacing cells after a VS16 emoji (ratatui#2721);
//! delete the shim once a newer crate ships, but keep `init`/`restore`. See docs/tui.md, "Contracts".
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};
use std::io::{self, Stdout, Write, stdout};
use unicode_width::UnicodeWidthStr;

pub struct WideCellBackend<W: Write> {
    inner: CrosstermBackend<W>,
}

impl<W: Write> WideCellBackend<W> {
    pub fn new(writer: W) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
        }
    }
}

fn cell_width(cell: &Cell) -> u16 {
    u16::try_from(cell.symbol().width()).unwrap_or(u16::MAX)
}

fn has_vs16(cell: &Cell) -> bool {
    cell.symbol().contains('\u{FE0F}')
}

impl<W: Write> Write for WideCellBackend<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

impl<W: Write> Backend for WideCellBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let mut batch: Vec<(u16, u16, &'a Cell)> = Vec::new();
        let mut cover: Option<(u16, u16)> = None;

        for (x, y, cell) in content {
            if let Some((cy, cx_end)) = cover.take()
                && y == cy
                && x < cx_end
                && !batch.is_empty()
            {
                self.inner.draw(batch.drain(..))?;
            }
            let w = cell_width(cell);
            if w > 1 && has_vs16(cell) {
                cover = Some((y, x.saturating_add(w)));
            }
            batch.push((x, y, cell));
        }
        if !batch.is_empty() {
            self.inner.draw(batch.drain(..))?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, p: P) -> io::Result<()> {
        self.inner.set_cursor_position(p)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, t: ClearType) -> io::Result<()> {
        self.inner.clear_region(t)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }
}

/// `ratatui::init()` with our backend; its panic hook runs before `panic = "abort"`, hence no
/// Drop guard. `paste` turns on bracketed paste, so a paste arrives as one event, not as keys.
pub fn init(paste: bool) -> io::Result<Terminal<WideCellBackend<Stdout>>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore(paste);
        hook(info);
    }));
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    if paste {
        execute!(stdout(), EnableBracketedPaste)?;
    }
    Terminal::new(WideCellBackend::new(stdout()))
}

/// The fd crossterm reads keys from: stdin when it is a tty, else /dev/tty,
/// held open by the returned file.
fn input_fd() -> Option<(Option<std::fs::File>, libc::c_int)> {
    // SAFETY: isatty only inspects the descriptor.
    if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
        return Some((None, libc::STDIN_FILENO));
    }
    let f = std::fs::File::open("/dev/tty").ok()?;
    let fd = std::os::fd::AsRawFd::as_raw_fd(&f);
    Some((Some(f), fd))
}

/// Drops every key typed but not yet read. Best effort: no tty, nothing to drop.
pub fn flush_input() {
    if let Some((_keep, fd)) = input_fd() {
        // SAFETY: tcflush only discards the descriptor's queued input.
        unsafe { libc::tcflush(fd, libc::TCIFLUSH) };
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Watch {
    HungUp,
    /// POLLNVAL: Darwin's poll(2) does not take devices, so no watch is possible.
    Unwatchable,
    Failed,
}

/// Blocks until `fd` hangs up. events 0: poll wakes only for POLLHUP, POLLERR or POLLNVAL.
pub fn wait_hangup(fd: libc::c_int) -> Watch {
    let mut p = libc::pollfd { fd, events: 0, revents: 0 };
    loop {
        // SAFETY: one live pollfd on the stack.
        let r = unsafe { libc::poll(&mut p, 1, -1) };
        if r < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Watch::Failed;
        }
        if p.revents & libc::POLLNVAL != 0 {
            return Watch::Unwatchable;
        }
        if p.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            return Watch::HungUp;
        }
    }
}

/// Exits the process once the terminal hangs up. crossterm 0.29 rereads a hung-up
/// tty forever (EOF and EIO are neither WouldBlock nor EINTR), so the loop never runs again.
pub fn exit_on_hangup(cleanup: fn()) {
    std::thread::spawn(move || {
        let Some((_keep, fd)) = input_fd() else { return };
        match wait_hangup(fd) {
            Watch::HungUp => {}
            w => return crate::data::log(&format!("no hangup watch on the terminal: {w:?}")),
        }
        crate::data::log("the terminal hung up: exiting");
        cleanup();
        // SAFETY: nothing is left to flush to a terminal that is gone.
        unsafe { libc::_exit(0) };
    });
}

/// `ratatui::restore()`, plus the bracketed paste `init(true)` turned on.
pub fn restore(paste: bool) {
    if paste {
        let _ = execute!(stdout(), DisableBracketedPaste);
    }
    ratatui::restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;

    /// The absolute move Crossterm emits for `MoveTo(x, y)`.
    fn move_to(x: u16, y: u16) -> String {
        format!("\x1b[{};{}H", y + 1, x + 1)
    }

    fn render(content: Vec<(u16, u16, Cell)>) -> Vec<u8> {
        let mut sink: Vec<u8> = Vec::new();
        {
            let refs: Vec<(u16, u16, &Cell)> = content.iter().map(|(x, y, c)| (*x, *y, c)).collect();
            let mut b = WideCellBackend::new(&mut sink);
            b.draw(refs.into_iter()).expect("draw into an in-memory writer");
        }
        sink
    }

    fn cell(symbol: &str) -> Cell {
        let mut c = Cell::default();
        c.set_symbol(symbol);
        c
    }

    fn count(hay: &[u8], needle: &str) -> usize {
        let n = needle.as_bytes();
        let (mut i, mut total) = (0, 0);
        while i + n.len() <= hay.len() {
            if &hay[i..i + n.len()] == n {
                total += 1;
                i += n.len();
            } else {
                i += 1;
            }
        }
        total
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        count(hay, needle) > 0
    }

    #[test]
    fn the_watch_fires_on_a_hangup_and_never_on_an_fd_it_cannot_poll() {
        let mut fds = [0; 2];
        // SAFETY: a fresh pipe; both ends are closed below.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let rd = fds[0];
        let w = std::thread::spawn(move || wait_hangup(rd));
        // SAFETY: the write end is ours; closing it hangs up the read end.
        unsafe { libc::close(fds[1]) };
        assert_eq!(w.join().unwrap(), Watch::HungUp);
        // SAFETY: as above.
        unsafe { libc::close(rd) };
        // far past any open fd, so no parallel test can own it: POLLNVAL, like a tty on Darwin
        assert_eq!(wait_hangup(999_999), Watch::Unwatchable);
    }

    #[test]
    fn a_pty_whose_master_closes_hangs_up() {
        let (mut m, mut s) = (0, 0);
        // SAFETY: openpty writes two fds; the null name, termios and size are allowed.
        let r = unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
        assert_eq!(r, 0);
        let w = std::thread::spawn(move || wait_hangup(s));
        // SAFETY: the master is ours.
        unsafe { libc::close(m) };
        assert_eq!(w.join().unwrap(), Watch::HungUp);
        // SAFETY: as above.
        unsafe { libc::close(s) };
    }

    #[test]
    fn vs16_trailing_cell_gets_an_absolute_move_and_keeps_its_style() {
        // ⌨️ (width 2) at x=0 covers x=1, where the changed cell is bold and
        // non-space, so a fabricated Print(" ") would show in the bytes.
        let glyph = "\u{2328}\u{FE0F}";
        let mut z = cell("z");
        z.modifier = Modifier::BOLD;
        let out = render(vec![(0, 0, cell(glyph)), (1, 0, z), (2, 0, cell("x"))]);

        assert!(contains(&out, &move_to(0, 0)), "glyph start needs its own move");
        assert!(contains(&out, glyph), "the glyph must be printed");
        assert_eq!(count(&out, &move_to(1, 0)), 1, "exactly one MoveTo(1,0) for the trailing cell");
        assert!(!contains(&out, &move_to(2, 0)), "x=2 rides the relative advance from x=1");
        assert_eq!(count(&out, "z"), 1, "the real trailing symbol survives, exactly once");
        assert!(contains(&out, "\x1b[1m"), "the trailing cell's bold reaches the wire");
    }

    /// An ordinary wide glyph is already safe: the diff yields x+2, which is
    /// non-contiguous, so the inner backend emits the move by itself.
    #[test]
    fn cjk_is_left_alone_and_nothing_is_dropped() {
        let out = render(vec![(1, 0, cell("中")), (3, 0, cell("x"))]);
        assert!(contains(&out, "中"), "the CJK glyph must be printed");
        assert!(contains(&out, "x"), "the following cell must not be dropped");
        assert!(contains(&out, &move_to(3, 0)), "x+2 is non-contiguous, so it gets a move");
    }

    #[test]
    fn ascii_run_uses_only_the_initial_move() {
        let out = render(vec![(0, 0, cell("a")), (1, 0, cell("b")), (2, 0, cell("c"))]);
        assert!(contains(&out, &move_to(0, 0)), "initial MoveTo(0,0)");
        assert!(!contains(&out, &move_to(1, 0)), "a contiguous run must not re-position");
        assert!(!contains(&out, &move_to(2, 0)), "a contiguous run must not re-position");
    }
}
