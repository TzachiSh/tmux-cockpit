// cockpit-fleet: a Bubble Tea spike of tmux-cockpit's fleet view.
// Reads the tmux server named by COCKPIT_TMUX (default: "tmux"), never writes
// except switch-client on Enter. One tick per second: list-panes, the
// transcript tail per agent pane, capture-pane for the selected row.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
)

var tmuxCmd = strings.Fields(envOr("COCKPIT_TMUX", "tmux"))
var agentRe = regexp.MustCompile(`^(claude|codex|aider|cursor|opencode|gemini|crush)$`)

func envOr(k, d string) string {
	if v := os.Getenv(k); v != "" {
		return v
	}
	return d
}

func tmux(args ...string) (string, error) {
	c := exec.Command(tmuxCmd[0], append(tmuxCmd[1:], args...)...)
	out, err := c.Output()
	return string(out), err
}

type pane struct {
	id, where, name, cmd, state, title, msg, sess, path, pid string
	doing, said                                              string
	since                                                    time.Duration
}

// --- transcript tail -------------------------------------------------------

type line struct {
	Type      string `json:"type"`
	Timestamp string `json:"timestamp"`
	Message   struct {
		Content json.RawMessage `json:"content"`
	} `json:"message"`
}
type block struct {
	Type  string `json:"type"`
	Name  string `json:"name"`
	Text  string `json:"text"`
	Input struct {
		Description string `json:"description"`
		Prompt      string `json:"prompt"`
		Command     string `json:"command"`
		FilePath    string `json:"file_path"`
	} `json:"input"`
}

func transcriptPath(sess, path string) string {
	home, _ := os.UserHomeDir()
	slug := strings.NewReplacer("/", "-", ".", "-").Replace(path)
	return filepath.Join(home, ".claude", "projects", slug, sess+".jsonl")
}

// tail reads the last 64 KB and keeps whole lines.
func tail(p string) []string {
	f, err := os.Open(p)
	if err != nil {
		return nil
	}
	defer f.Close()
	st, _ := f.Stat()
	off := st.Size() - 65536
	if off < 0 {
		off = 0
	}
	f.Seek(off, io.SeekStart)
	r := bufio.NewReader(f)
	if off > 0 {
		r.ReadString('\n') // drop the partial first line
	}
	var out []string
	sc := bufio.NewScanner(r)
	sc.Buffer(make([]byte, 1<<20), 1<<20)
	for sc.Scan() {
		out = append(out, sc.Text())
	}
	return out
}

func (p *pane) readDoing() {
	if p.sess == "" {
		return
	}
	var lastTool, lastText string
	var toolAt time.Time
	for _, raw := range tail(transcriptPath(p.sess, p.path)) {
		var l line
		if json.Unmarshal([]byte(raw), &l) != nil || l.Type != "assistant" {
			continue
		}
		var blocks []block
		if json.Unmarshal(l.Message.Content, &blocks) != nil {
			continue
		}
		for _, b := range blocks {
			switch b.Type {
			case "tool_use":
				d := b.Input.Description
				if d == "" {
					d = b.Input.Prompt
				}
				if d == "" {
					d = b.Input.Command
				}
				if d == "" {
					d = b.Input.FilePath
				}
				lastTool = b.Name + ": " + strings.SplitN(d, "\n", 2)[0]
				toolAt, _ = time.Parse(time.RFC3339Nano, l.Timestamp)
			case "text":
				if t := strings.TrimSpace(b.Text); t != "" {
					lastText = strings.SplitN(t, "\n", 2)[0]
				}
			}
		}
	}
	p.doing, p.said = lastTool, lastText
	if !toolAt.IsZero() {
		p.since = time.Since(toolAt).Round(time.Second)
	}
}

// --- claude agents --json fallback for panes with no @cockpit_session ---------

func agentSessions() map[string]string { // pid -> sessionId
	out, err := exec.Command("claude", "agents", "--json").Output()
	m := map[string]string{}
	if err != nil {
		return m
	}
	var rows []struct {
		Pid       int    `json:"pid"`
		SessionID string `json:"sessionId"`
	}
	json.Unmarshal(out, &rows)
	for _, r := range rows {
		m[strconv.Itoa(r.Pid)] = r.SessionID
	}
	return m
}

func descendants() map[string][]string { // ppid -> pids
	out, _ := exec.Command("ps", "-eo", "pid=,ppid=").Output()
	m := map[string][]string{}
	for _, l := range strings.Split(string(out), "\n") {
		f := strings.Fields(l)
		if len(f) == 2 {
			m[f[1]] = append(m[f[1]], f[0])
		}
	}
	return m
}

func findSession(panePid string, kids map[string][]string, agents map[string]string) string {
	q := []string{panePid}
	for len(q) > 0 {
		p := q[0]
		q = q[1:]
		if s, ok := agents[p]; ok {
			return s
		}
		q = append(q, kids[p]...)
	}
	return ""
}

// --- the tick -----------------------------------------------------------------

func listPanes() []pane {
	out, err := tmux("list-panes", "-a", "-F",
		"#{pane_id}\t#{session_name}:#{window_index}\t#{window_name}\t#{pane_current_command}\t#{@panestate}\t#{@cockpit_title}\t#{@panestate_msg}\t#{@cockpit_session}\t#{pane_current_path}\t#{pane_pid}")
	if err != nil {
		return nil
	}
	var ps []pane
	var agents map[string]string
	var kids map[string][]string
	for _, l := range strings.Split(strings.TrimRight(out, "\n"), "\n") {
		f := strings.Split(l, "\t")
		if len(f) < 10 || !agentRe.MatchString(f[3]) {
			continue
		}
		p := pane{id: f[0], where: f[1], name: f[2], cmd: f[3], state: f[4], title: f[5], msg: f[6], sess: f[7], path: f[8], pid: f[9]}
		if p.sess == "" {
			if agents == nil {
				agents, kids = agentSessions(), descendants()
			}
			p.sess = findSession(p.pid, kids, agents)
		}
		p.readDoing()
		ps = append(ps, p)
	}
	return ps
}

func preview(id string, n int) string {
	out, _ := tmux("capture-pane", "-ep", "-t", id, "-S", "-"+strconv.Itoa(n))
	lines := strings.Split(strings.TrimRight(out, "\n"), "\n")
	if len(lines) > n {
		lines = lines[len(lines)-n:]
	}
	return strings.Join(lines, "\n")
}

// --- Bubble Tea ---------------------------------------------------------------

type tickMsg time.Time
type panesMsg struct {
	panes   []pane
	preview string
	took    time.Duration
}

type model struct {
	panes         []pane
	cur           int
	preview       string
	w, h          int
	took          time.Duration
	ticks         int
	quitting      bool
}

func (m model) Init() tea.Cmd { return tea.Batch(m.refresh(), tick()) }
func tick() tea.Cmd          { return tea.Tick(time.Second, func(t time.Time) tea.Msg { return tickMsg(t) }) }
func (m model) refresh() tea.Cmd {
	cur := m.cur
	h := m.h
	return func() tea.Msg {
		t0 := time.Now()
		ps := listPanes()
		pv := ""
		if cur < len(ps) && h > 0 {
			rows := 3
			for _, p := range ps {
				rows++
				if p.said != "" {
					rows++
				}
			}
			pv = preview(ps[cur].id, max(3, h-rows-2))
		}
		return panesMsg{ps, pv, time.Since(t0)}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
		if lf := os.Getenv("COCKPIT_LOG"); lf != "" {
			f, _ := os.OpenFile(lf, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
			fmt.Fprintf(f, "size %dx%d TERM=%s\n", m.w, m.h, os.Getenv("TERM"))
			f.Close()
		}
		return m, m.refresh()
	case tickMsg:
		m.ticks++
		return m, tea.Batch(m.refresh(), tick())
	case panesMsg:
		m.panes, m.preview, m.took = msg.panes, msg.preview, msg.took
		if lf := os.Getenv("COCKPIT_LOG"); lf != "" {
			f, _ := os.OpenFile(lf, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
			fmt.Fprintf(f, "tick %d panes took %s\n", len(m.panes), m.took)
			f.Close()
		}
		if m.cur >= len(m.panes) {
			m.cur = max(0, len(m.panes)-1)
		}
		return m, nil
	case tea.KeyMsg:
		switch msg.String() {
		case "q", "esc", "ctrl+c":
			m.quitting = true
			return m, tea.Quit
		case "j", "down":
			if m.cur < len(m.panes)-1 {
				m.cur++
			}
			return m, m.refresh()
		case "k", "up":
			if m.cur > 0 {
				m.cur--
			}
			return m, m.refresh()
		case "enter":
			if m.cur < len(m.panes) {
				tmux("switch-client", "-t", m.panes[m.cur].id)
			}
			m.quitting = true
			return m, tea.Quit
		}
	}
	return m, nil
}

var (
	glyph = map[string]string{"needs-input": "▲", "error": "✕", "running": "●", "done": "✓", "": "○"}
	col   = map[string]lipgloss.Color{"needs-input": "1", "error": "3", "running": "3", "done": "2", "": "8"}
	sel   = lipgloss.NewStyle().Reverse(true)
	dim   = lipgloss.NewStyle().Foreground(lipgloss.Color("8"))
	box   = lipgloss.NewStyle().Border(lipgloss.NormalBorder(), true, false, false, false).BorderForeground(lipgloss.Color("8"))
)

func trunc(s string, n int) string {
	r := []rune(s)
	if len(r) <= n {
		return s
	}
	return string(r[:n-1]) + "…"
}

func (m model) View() string {
	if m.quitting {
		return ""
	}
	var b strings.Builder
	fmt.Fprintf(&b, " fleet · %d agents · tick %s · j/k move · ↵ jump · q close\n\n", len(m.panes), m.took.Round(time.Millisecond))
	for i, p := range m.panes {
		g := lipgloss.NewStyle().Foreground(col[p.state]).Render(glyph[p.state])
		title := p.title
		if title == "" {
			title = p.name
		}
		doing := p.doing
		if p.state == "needs-input" && p.msg != "" {
			doing = "needs you: " + p.msg
		}
		age := ""
		if p.since > 0 && p.state == "running" {
			age = dim.Render(p.since.String())
		}
		row := fmt.Sprintf(" %s %-4s %-12s %-22s %s %s", g, p.id, trunc(p.where, 12), trunc(title, 22), trunc(doing, max(10, m.w-50)), age)
		if i == m.cur {
			row = sel.Render(row)
		}
		b.WriteString(row + "\n")
		if p.said != "" {
			b.WriteString(dim.Render("        "+trunc(p.said, max(10, m.w-10))) + "\n")
		}
	}
	if m.preview != "" {
		cut := lipgloss.NewStyle().MaxWidth(max(10, m.w-1))
		var pl []string
		for _, l := range strings.Split(m.preview, "\n") {
			pl = append(pl, cut.Render(l))
		}
		b.WriteString(box.Render(strings.Join(pl, "\n")))
	}
	return b.String()
}

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--rows" { // headless: print the rows once
		for _, p := range listPanes() {
			fmt.Printf("%s\t%s\t%s\t%s\t%s\t%s\t%s\n", glyph[p.state], p.id, p.where, p.title, p.doing, p.since, p.said)
		}
		return
	}
	if _, err := tea.NewProgram(model{}, tea.WithAltScreen()).Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
