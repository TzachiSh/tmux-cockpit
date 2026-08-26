// cockpit-fleet-rs: a Ratatui spike of tmux-cockpit's fleet view, the twin of
// the Bubble Tea one. Reads the tmux server named by COCKPIT_TMUX (default
// "tmux"), writes nothing except switch-client on Enter. One tick per second.
use ansi_to_tui::IntoText;
use ratatui::crossterm::event::{self, Event, KeyCode};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

fn tmux_cmd() -> Vec<String> {
    std::env::var("COCKPIT_TMUX").unwrap_or_else(|_| "tmux".into()).split_whitespace().map(String::from).collect()
}
fn tmux(args: &[&str]) -> String {
    let c = tmux_cmd();
    Command::new(&c[0]).args(&c[1..]).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}
fn log(s: &str) {
    if let Ok(p) = std::env::var("COCKPIT_LOG") {
        if let Ok(mut f) = OpenOptions::new().append(true).create(true).open(p) { let _ = writeln!(f, "{s}"); }
    }
}

#[derive(Default, Clone)]
struct Pane { id: String, wher: String, name: String, state: String, title: String, msg: String, sess: String, path: String, pid: String, doing: String, said: String, since: Option<Duration> }

fn transcript_path(sess: &str, path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let slug = path.replace(['/', '.'], "-");
    format!("{home}/.claude/projects/{slug}/{sess}.jsonl")
}

/// last 64 KB, whole lines only
fn tail(p: &str) -> Vec<String> {
    let Ok(mut f) = File::open(p) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let off = len.saturating_sub(65536);
    let _ = f.seek(SeekFrom::Start(off));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    let mut lines: Vec<String> = s.lines().map(String::from).collect();
    if off > 0 && !lines.is_empty() { lines.remove(0); }
    lines
}

fn read_doing(p: &mut Pane) {
    if p.sess.is_empty() { return; }
    let (mut tool, mut text, mut at) = (String::new(), String::new(), None::<SystemTime>);
    for raw in tail(&transcript_path(&p.sess, &p.path)) {
        let Ok(v) = serde_json::from_str::<Value>(&raw) else { continue };
        if v["type"] != "assistant" { continue }
        let Some(blocks) = v["message"]["content"].as_array() else { continue };
        for b in blocks {
            match b["type"].as_str() {
                Some("tool_use") => {
                    let i = &b["input"];
                    let d = ["description", "prompt", "command", "file_path"].iter().find_map(|k| i[k].as_str().filter(|s| !s.is_empty())).unwrap_or("");
                    tool = format!("{}: {}", b["name"].as_str().unwrap_or(""), d.lines().next().unwrap_or(""));
                    at = v["timestamp"].as_str().and_then(|t| httpdate_like(t));
                }
                Some("text") => { if let Some(t) = b["text"].as_str().map(str::trim).filter(|t| !t.is_empty()) { text = t.lines().next().unwrap_or("").to_string(); } }
                _ => {}
            }
        }
    }
    p.doing = tool; p.said = text;
    p.since = at.and_then(|t| SystemTime::now().duration_since(t).ok());
}
/// RFC 3339 "2026-08-26T18:27:47.123Z" to SystemTime, no chrono
fn httpdate_like(t: &str) -> Option<SystemTime> {
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (d.next()??, d.next()??, d.next()??);
    let time = time.trim_end_matches('Z');
    let mut tt = time.split(':');
    let (h, mi) = (tt.next()?.parse::<i64>().ok()?, tt.next()?.parse::<i64>().ok()?);
    let s = tt.next()?.parse::<f64>().ok()?;
    // days from civil (Howard Hinnant)
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400); let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + dd - 1; let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + h * 3600 + mi * 60 + s as i64;
    Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64))
}

fn agent_sessions() -> HashMap<String, String> {
    let out = Command::new("claude").args(["agents", "--json"]).output().map(|o| o.stdout).unwrap_or_default();
    let v: Value = serde_json::from_slice(&out).unwrap_or(Value::Null);
    v.as_array().map(|a| a.iter().filter_map(|r| Some((r["pid"].as_i64()?.to_string(), r["sessionId"].as_str()?.to_string()))).collect()).unwrap_or_default()
}
fn descendants() -> HashMap<String, Vec<String>> {
    let out = Command::new("ps").args(["-eo", "pid=,ppid="]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut m: HashMap<String, Vec<String>> = HashMap::new();
    for l in out.lines() { let f: Vec<&str> = l.split_whitespace().collect(); if f.len() == 2 { m.entry(f[1].into()).or_default().push(f[0].into()); } }
    m
}
fn find_session(pid: &str, kids: &HashMap<String, Vec<String>>, agents: &HashMap<String, String>) -> String {
    let mut q = vec![pid.to_string()];
    while let Some(p) = q.pop() {
        if let Some(s) = agents.get(&p) { return s.clone(); }
        if let Some(k) = kids.get(&p) { q.extend(k.iter().cloned()); }
    }
    String::new()
}

fn list_panes() -> Vec<Pane> {
    let out = tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{session_name}:#{window_index}\t#{window_name}\t#{pane_current_command}\t#{@panestate}\t#{@cockpit_title}\t#{@panestate_msg}\t#{@cockpit_session}\t#{pane_current_path}\t#{pane_pid}"]);
    let agents_re = ["claude", "codex", "aider", "cursor", "opencode", "gemini", "crush"];
    let (mut agents, mut kids) = (None, None);
    let mut ps = vec![];
    for l in out.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 10 || !agents_re.contains(&f[3]) { continue }
        let mut p = Pane { id: f[0].into(), wher: f[1].into(), name: f[2].into(), state: f[4].into(), title: f[5].into(), msg: f[6].into(), sess: f[7].into(), path: f[8].into(), pid: f[9].into(), ..Default::default() };
        if p.sess.is_empty() {
            if agents.is_none() { agents = Some(agent_sessions()); kids = Some(descendants()); }
            p.sess = find_session(&p.pid, kids.as_ref().unwrap(), agents.as_ref().unwrap());
        }
        read_doing(&mut p);
        ps.push(p);
    }
    ps
}
fn preview(id: &str, n: usize) -> String {
    let out = tmux(&["capture-pane", "-ep", "-t", id, "-S", &format!("-{n}")]);
    let lines: Vec<&str> = out.trim_end().lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn glyph(state: &str) -> (&'static str, Color) {
    match state { "needs-input" => ("▲", Color::Red), "error" => ("✕", Color::Yellow), "running" => ("●", Color::Yellow), "done" => ("✓", Color::Green), _ => ("○", Color::DarkGray) }
}
fn fmt_dur(d: Duration) -> String { let s = d.as_secs(); if s >= 3600 { format!("{}h{}m", s / 3600, s % 3600 / 60) } else if s >= 60 { format!("{}m{}s", s / 60, s % 60) } else { format!("{s}s") } }
fn trunc(s: &str, n: usize) -> String { let c: Vec<char> = s.chars().collect(); if c.len() <= n { s.into() } else { c[..n.saturating_sub(1)].iter().collect::<String>() + "…" } }

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--rows") {
        for p in list_panes() {
            println!("{}\t{}\t{}\t{}\t{}\t{}\t{}", glyph(&p.state).0, p.id, p.wher, p.title, p.doing, p.since.map(fmt_dur).unwrap_or_default(), p.said);
        }
        return;
    }
    let mut term = ratatui::init(); // raw mode + alt screen + panic hook that restores the terminal
    let (mut panes, mut cur, mut pv, mut took) = (list_panes(), 0usize, String::new(), Duration::ZERO);
    let mut last = Instant::now() - Duration::from_secs(2);
    let mut jump = None;
    loop {
        let size = term.size().unwrap_or_default();
        if last.elapsed() >= Duration::from_secs(1) {
            let t0 = Instant::now();
            panes = list_panes();
            if cur >= panes.len() { cur = panes.len().saturating_sub(1); }
            let rows: usize = panes.iter().map(|p| if p.said.is_empty() { 1 } else { 2 }).sum::<usize>() + 3;
            pv = panes.get(cur).map(|p| preview(&p.id, (size.height as usize).saturating_sub(rows + 2).max(3))).unwrap_or_default();
            took = t0.elapsed(); last = Instant::now();
            log(&format!("size {}x{} TERM={} tick {} panes took {:?}", size.width, size.height, std::env::var("TERM").unwrap_or_default(), panes.len(), took));
        }
        let _ = term.draw(|f| {
            let w = f.area().width as usize;
            let mut lines = vec![Line::from(format!(" fleet · {} agents · tick {:?} · j/k move · ↵ jump · q close", panes.len(), took)), Line::from("")];
            for (i, p) in panes.iter().enumerate() {
                let (g, c) = glyph(&p.state);
                let title = if p.title.is_empty() { &p.name } else { &p.title };
                let doing = if p.state == "needs-input" && !p.msg.is_empty() { format!("needs you: {}", p.msg) } else { p.doing.clone() };
                let age = match (p.since, p.state.as_str()) { (Some(d), "running") => fmt_dur(d), _ => String::new() };
                let mut spans = vec![Span::raw(" "), Span::styled(g, Style::default().fg(c)), Span::raw(format!(" {:<4} {:<12} {:<22} {} ", p.id, trunc(&p.wher, 12), trunc(title, 22), trunc(&doing, w.saturating_sub(50).max(10)))), Span::styled(age, Style::default().fg(Color::DarkGray))];
                if i == cur { for s in spans.iter_mut() { s.style = s.style.add_modifier(Modifier::REVERSED); } }
                lines.push(Line::from(spans));
                if !p.said.is_empty() { lines.push(Line::from(Span::styled(format!("        {}", trunc(&p.said, w.saturating_sub(10).max(10))), Style::default().fg(Color::DarkGray)))); }
            }
            let n = lines.len() as u16;
            let [top, bottom] = Layout::vertical([Constraint::Length(n), Constraint::Min(3)]).areas(f.area());
            f.render_widget(Paragraph::new(lines), top);
            let text = pv.as_bytes().into_text().unwrap_or_default();
            f.render_widget(Paragraph::new(text).block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray))), bottom);
        });
        if event::poll(Duration::from_millis(200)).unwrap_or(false) {
            if let Ok(Event::Key(k)) = event::read() {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('j') | KeyCode::Down => { if cur + 1 < panes.len() { cur += 1; last -= Duration::from_secs(1); } }
                    KeyCode::Char('k') | KeyCode::Up => { if cur > 0 { cur -= 1; last -= Duration::from_secs(1); } }
                    KeyCode::Enter => { jump = panes.get(cur).map(|p| p.id.clone()); break; }
                    _ => {}
                }
            }
        }
    }
    ratatui::restore();
    if let Some(id) = jump { tmux(&["switch-client", "-t", &id]); }
}
