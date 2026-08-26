/* cockpit-fleet-c: the fleet screen in ncursesw. A state change repaints on a
   signal instead of on the next tick: SIGUSR1 (pid in @cockpit_fleetc_pid) or
   SIGWINCH, with a 1 s poll(2) timeout as the floor.
   Grown from scratchpad/more/c/fleet.c (93 lines, poll-only, plain preview). */
/* feature macros come from `pkg-config --cflags ncursesw` on the gcc line */
#include <locale.h>
#include <ncursesw/curses.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <unistd.h>
#include <wchar.h>

#define MAXR 128
#define MAXC 8   /* glyph id where title doing age repo dirty */
static char *cell[MAXR][MAXC];
static int nrow;
static char rowbuf[65536];
static unsigned rowsum;

static volatile sig_atomic_t sigwake, sigquit;
static FILE *lgf;

static const char *tmuxcmd(void) {
	const char *t = getenv("COCKPIT_TMUX");
	return t && *t ? t : "tmux";
}

static void lg(const char *fmt, ...) {
	if (!lgf) return;
	struct timeval tv;
	gettimeofday(&tv, NULL);
	fprintf(lgf, "%ld.%06ld ", (long)tv.tv_sec, (long)tv.tv_usec);
	va_list ap;
	va_start(ap, fmt);
	vfprintf(lgf, fmt, ap);
	va_end(ap);
	fputc('\n', lgf);
	fflush(lgf);
}

static void run(const char *cmd, char *buf, size_t n) {
	FILE *f = popen(cmd, "r");
	size_t got = 0, r;
	if (f) {
		while (got < n - 1 && (r = fread(buf + got, 1, n - 1 - got, f)) > 0) got += r;
		pclose(f);
	}
	buf[got] = 0;
}

/* ---- rows ---- */
static void load(void) {
	char cmd[512];
	const char *rows = getenv("COCKPIT_ROWS");
	if (rows && *rows)
		snprintf(cmd, sizeof cmd, "COCKPIT_TMUX=\"%s\" %s --rows", tmuxcmd(), rows);
	else /* fallback: tmux does the formatting itself */
		snprintf(cmd, sizeof cmd,
		         "%s list-panes -a -F '#{?#{==:#{@panestate},needs-input},\xe2\x96\xb2,"
		         "#{?#{==:#{@panestate},done},\xe2\x9c\x93,\xe2\x97\x8f}}\t#{pane_id}\t"
		         "#{session_name}:#{window_index}\t#{@cockpit_title}\t#{@panestate_msg}\t\t\t'",
		         tmuxcmd());
	run(cmd, rowbuf, sizeof rowbuf);
	rowsum = 5381;
	for (char *p = rowbuf; *p; p++) rowsum = rowsum * 33u + (unsigned char)*p;
	for (int i = 0; i < nrow; i++)
		for (int j = 0; j < MAXC; j++) { free(cell[i][j]); cell[i][j] = NULL; }
	nrow = 0;
	for (char *l = strtok(rowbuf, "\n"); l && nrow < MAXR; l = strtok(NULL, "\n")) {
		int c = 0;
		for (char *p = l; c < MAXC; c++) {
			char *t = strchr(p, '\t');
			if (t) *t = 0;
			cell[nrow][c] = strdup(p);
			if (!t) { c++; break; }
			p = t + 1;
		}
		for (; c < MAXC; c++) cell[nrow][c] = strdup("");
		nrow++;
	}
}

/* ---- colour ---- */
static int ncol;  /* palette size ncurses will actually accept */

static int cube256(int r, int g, int b) {  /* truecolour -> xterm cube */
	if (r == g && g == b) return r < 8 ? 16 : r > 248 ? 231 : 232 + (r - 8) / 10;
	return 16 + 36 * (r * 5 / 255) + 6 * (g * 5 / 255) + (b * 5 / 255);
}

static short pair_of(int fg, int bg) {
	static struct { int fg, bg; short p; } c[240];
	static int n; static short next = 16;  /* 1..15 reserved for the UI */
	if (fg >= ncol) fg = fg % 8;
	if (bg >= ncol) bg = bg % 8;
	for (int i = 0; i < n; i++) if (c[i].fg == fg && c[i].bg == bg) return c[i].p;
	if (n == 240 || next >= COLOR_PAIRS) return 0;
	init_pair(next, (short)fg, (short)bg);
	c[n].fg = fg; c[n].bg = bg; c[n].p = next;
	n++;
	return next++;
}

/* ---- SGR translator: ESC[..m from capture-pane -ep -> ncurses attrs ---- */
static int sgr_fg, sgr_bg, sgr_at;

static void sgr_reset(void) { sgr_fg = -1; sgr_bg = -1; sgr_at = 0; }

static void sgr_apply(const int *p, int n) {
	for (int i = 0; i < n; i++) {
		int v = p[i];
		if (v == 0) sgr_reset();
		else if (v == 1) sgr_at |= A_BOLD;
		else if (v == 3) sgr_at |= A_ITALIC;
		else if (v == 4) sgr_at |= A_UNDERLINE;
		else if (v == 7) sgr_at |= A_REVERSE;
		else if (v == 22) sgr_at &= ~A_BOLD;
		else if (v == 27) sgr_at &= ~A_REVERSE;
		else if (v >= 30 && v <= 37) sgr_fg = v - 30;
		else if (v >= 40 && v <= 47) sgr_bg = v - 40;
		else if (v >= 90 && v <= 97) sgr_fg = v - 90 + 8;
		else if (v >= 100 && v <= 107) sgr_bg = v - 100 + 8;
		else if (v == 39) sgr_fg = -1;
		else if (v == 49) sgr_bg = -1;
		else if ((v == 38 || v == 48) && i + 1 < n) {
			int *tgt = v == 38 ? &sgr_fg : &sgr_bg;
			if (p[i + 1] == 5 && i + 2 < n) { *tgt = p[i + 2]; i += 2; }
			else if (p[i + 1] == 2 && i + 4 < n) { *tgt = cube256(p[i+2], p[i+3], p[i+4]); i += 4; }
			else i++;
		}
	}
}

static attr_t sgr_attr(void) {
	return sgr_at | (sgr_fg >= 0 || sgr_bg >= 0 ? COLOR_PAIR(pair_of(sgr_fg, sgr_bg)) : 0);
}

/* one wide char at (y,x), clipped to cols; returns columns consumed */
static int put_wch(int y, int x, wchar_t wc, attr_t a, int cols) {
	int w = wcwidth(wc);
	if (w < 0) return 0;
	if (w == 0) w = 1;
	if (x + w > cols) return -1;
	cchar_t cc;
	wchar_t s[2] = { wc, 0 };
	/* setcchar takes the pair as its own argument; the pair bits inside
	   `a` are ignored, which silently paints the whole screen monochrome. */
	setcchar(&cc, s, a & A_ATTRIBUTES, (short)PAIR_NUMBER(a), NULL);
	mvadd_wch(y, x, &cc);
	return w;
}

/* draw one capture-pane line, translating SGR as it goes */
static void draw_ansi(int y, const char *s, int cols) {
	mbstate_t st;
	memset(&st, 0, sizeof st);
	int x = 0;
	while (*s) {
		if (*s == 27 && s[1] == '[') {
			const char *p = s + 2;
			int par[16], n = 0, v = 0, seen = 0;
			while (*p && !(*p >= '@' && *p <= '~')) {
				if (*p >= '0' && *p <= '9') { v = v * 10 + (*p - '0'); seen = 1; }
				else if (*p == ';') { if (n < 16) par[n++] = seen ? v : 0; v = 0; seen = 0; }
				p++;
			}
			if (n < 16) par[n++] = seen ? v : 0;
			if (*p == 'm') sgr_apply(par, n);
			s = *p ? p + 1 : p;
			continue;
		}
		if (*s == 27) { s += s[1] ? 2 : 1; continue; }  /* other escapes: skip */
		wchar_t wc;
		size_t k = mbrtowc(&wc, s, MB_CUR_MAX, &st);
		if (k == (size_t)-1 || k == (size_t)-2) { s++; memset(&st, 0, sizeof st); continue; }
		if (k == 0) k = 1;
		s += k;
		if (wc == L'\t') wc = L' ';
		int w = put_wch(y, x, wc, sgr_attr(), cols);
		if (w < 0) return;
		x += w;
	}
}

/* plain field, padded/clipped by display columns (not bytes) */
static int put_field(int y, int x, const char *s, int w, attr_t a, int cols) {
	mbstate_t st;
	memset(&st, 0, sizeof st);
	int used = 0;
	while (*s && used < w) {
		wchar_t wc;
		size_t k = mbrtowc(&wc, s, MB_CUR_MAX, &st);
		if (k == (size_t)-1 || k == (size_t)-2 || k == 0) break;
		s += k;
		int cw = wcwidth(wc);
		if (cw <= 0) continue;
		if (used + cw > w) break;
		if (put_wch(y, x + used, wc, a, cols) < 0) return x + w + 1;
		used += cw;
	}
	for (; used < w; used++)
		if (put_wch(y, x + used, L' ', a, cols) < 0) break;
	return x + w + 1;
}

/* ---- draw ---- */
static char pv[262144];

static void draw(int sel) {
	int rows, cols;
	erase();
	getmaxyx(stdscr, rows, cols);
	int split = nrow + 1;
	for (int i = 0; i < nrow; i++) {
		int need = strcmp(cell[i][0], "\xe2\x96\xb2") == 0;
		attr_t a = need ? COLOR_PAIR(1) : 0;
		if (i == sel) a |= A_REVERSE;
		int x = 0;
		for (int c = 0; c < cols; c++) put_wch(i, c, L' ', a, cols);
		x = put_field(i, x, cell[i][0], 2, a | (need ? A_BOLD : 0), cols);
		x = put_field(i, x, cell[i][1], 4, a | COLOR_PAIR(need ? 1 : 2), cols);
		x = put_field(i, x, cell[i][2], 10, a, cols);
		x = put_field(i, x, cell[i][3], 28, a | (i == sel ? 0 : A_BOLD), cols);
		x = put_field(i, x, cell[i][4], cols - x - 6 > 0 ? cols - x - 6 : 1, a, cols);
		put_field(i, cols - 5 > 0 ? cols - 5 : 0, cell[i][5], 5, a, cols);
	}
	attron(COLOR_PAIR(3));
	mvhline(split, 0, ACS_HLINE, cols);
	if (nrow) {
		char t[80];
		snprintf(t, sizeof t, " %s ", cell[sel][1]);
		mvaddstr(split, 2, t);
	}
	attroff(COLOR_PAIR(3));
	lg("rows sum=%u", rowsum);
	if (nrow) {
		char cmd[512];
		snprintf(cmd, sizeof cmd, "%s capture-pane -ep -t %s 2>/dev/null", tmuxcmd(), cell[sel][1]);
		run(cmd, pv, sizeof pv);
		sgr_reset();
		int n = 0;
		for (char *l = strtok(pv, "\n"); l && split + 1 + n < rows; l = strtok(NULL, "\n"))
			draw_ansi(split + 1 + n++, l, cols);
	}
	refresh();
	lg("draw sum=%u rows=%d sel=%d", rowsum, nrow, sel);
}

/* ---- push channel: signals only ----
   SIGUSR1 sets a flag. SIGWINCH is deliberately NOT handled here: ncurses owns
   it and turns it into KEY_RESIZE on the next input read, so the key loop picks
   it up. Installing our own would displace ncurses' and break resizing. */
static void onsig(int s) { if (s == SIGUSR1) sigwake = 1; else sigquit = 1; }

static void publish_pid(int on) {
	if (getenv("COCKPIT_READONLY")) return;   /* never set-option on a server we only read */
	char c[256];
	/* Unpublish on exit: a stale pid is worse than no pid. The notifier does
	   kill -USR1 on whatever this says, and SIGUSR1's default action is to
	   terminate -- so a recycled pid means signalling an unrelated process.
	   ponytail: still open on SIGKILL; the notifier should check the pid is
	   ours before signalling if that ever bites. */
	if (on) snprintf(c, sizeof c, "%s set-option -g @cockpit_fleetc_pid %d", tmuxcmd(), (int)getpid());
	else    snprintf(c, sizeof c, "%s set-option -gu @cockpit_fleetc_pid", tmuxcmd());
	if (system(c) != 0) lg("publish failed");
}

int main(void) {
	setlocale(LC_ALL, "");
	const char *lp = getenv("COCKPIT_LOG");
	if (lp && *lp) lgf = fopen(lp, "a");

	struct sigaction sa = { .sa_handler = onsig };
	sigaction(SIGUSR1, &sa, NULL);
	/* Quit cleanly on the popup's teardown signal so the exit path unpublishes
	   the pid. Without these, display-popup -E leaves a dead pid in the option
	   and the notifier's kill -USR1 lands on whatever recycled it. */
	sigaction(SIGHUP, &sa, NULL);
	sigaction(SIGTERM, &sa, NULL);
	sigaction(SIGINT, &sa, NULL);

	set_escdelay(25);  /* default 1000 ms: a bare Esc would take a second to quit */
	initscr(); cbreak(); noecho(); curs_set(0); keypad(stdscr, TRUE); nodelay(stdscr, TRUE);
	start_color(); use_default_colors();
	ncol = COLORS > 0 ? COLORS : 8;
	init_pair(1, COLOR_RED, -1);
	init_pair(2, COLOR_CYAN, -1);
	init_pair(3, COLOR_BLUE, -1);
	publish_pid(1);
	lg("start colors=%d pairs=%d pid=%d", COLORS, COLOR_PAIRS, (int)getpid());
	int sel = 0, dirty = 1;
	char jump[64] = "";
	while (!sigquit) {
		if (dirty) {
			load();
			lg("fetch sum=%u", rowsum);
			if (sel >= nrow) sel = nrow ? nrow - 1 : 0;
			draw(sel);
			dirty = 0;
		}
		/* Both wakeups can land *during* the refetch above, and neither one
		   leaves anything on stdin: SIGUSR1 sets our flag, SIGWINCH sets a flag
		   inside ncurses that only surfaces as KEY_RESIZE on the next read. So
		   check the flag and drain the queue BEFORE poll(), or poll blocks its
		   full second on a wakeup that already happened. Draining unconditionally
		   (not gated on POLLIN) is what makes the resize visible at all. */
		if (sigwake) { sigwake = 0; lg("recv SIGUSR1"); dirty = 1; }
		int ch;
		while ((ch = getch()) != ERR) {
			if (ch == KEY_RESIZE) { lg("recv SIGWINCH"); dirty = 1; }
			else if (ch == 'q' || ch == 27) sigquit = 1;
			else if (ch == 'j' || ch == KEY_DOWN) { if (sel + 1 < nrow) sel++; dirty = 1; }
			else if (ch == 'k' || ch == KEY_UP) { if (sel) sel--; dirty = 1; }
			else if ((ch == '\n' || ch == KEY_ENTER) && nrow) {
				snprintf(jump, sizeof jump, "%s", cell[sel][1]);
				sigquit = 1;
			}
		}
		if (dirty || sigquit) continue;
		struct pollfd fd = { STDIN_FILENO, POLLIN, 0 };
		if (poll(&fd, 1, 1000) == 0) { lg("recv tick"); dirty = 1; }  /* the 1 s floor */
		/* whatever poll woke us for is drained at the top of the next pass */
	}
	endwin();
	publish_pid(0);
	if (*jump) {
		const char *out = getenv("COCKPIT_ACTION");
		FILE *f = out && *out ? fopen(out, "w") : NULL;
		fprintf(f ? f : stderr, "jump %s\n", jump);
		if (f) fclose(f);
	}
	return 0;
}
