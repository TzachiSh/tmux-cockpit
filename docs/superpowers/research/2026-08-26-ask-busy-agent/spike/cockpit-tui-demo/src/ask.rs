//! `prefix ?`, the observer ask. screens.md §5: one prompt line, a spinner
//! while a tool-less haiku reads the transcript tail, a wrapped answer, a
//! cost/time footer. The observer never opens the agent's session for writing.
use crate::data::{Pane, excerpt, trunc, wrap};
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Paragraph};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

const SYSTEM: &str = "You are a read-only observer of another coding agent's session. Answer from the transcript excerpt only, two lines max.";
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct Answer {
    pub text: String,
    pub cost: f64,
    pub ms: u64,
}

/// Runs the observer. COCKPIT_ASK_STUB=1 substitutes a fixed reply so the tests
/// never spend money or need the network.
pub fn observe(sess: &str, path: &str, q: &str) -> Answer {
    let t0 = Instant::now();
    if std::env::var("COCKPIT_ASK_STUB").as_deref() == Ok("1") {
        std::thread::sleep(Duration::from_secs(2));
        return Answer { text: "stub".into(), cost: 0.05, ms: t0.elapsed().as_millis() as u64 };
    }
    let ctx = excerpt(sess, path, 24000);
    let prompt = format!("{ctx}\n\n---\nQuestion: {q}");
    // env -u CLAUDECODE: a nested claude refuses when launched from inside a
    // Claude pane or hook.
    let out = Command::new("env")
        .args([
            "-u",
            "CLAUDECODE",
            "claude",
            "-p",
            "--model",
            "haiku",
            "--tools",
            "",
            "--no-session-persistence",
            "--output-format",
            "json",
            "--append-system-prompt",
            SYSTEM,
            &prompt,
        ])
        .output();
    let stdout = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
    Answer {
        text: v["result"].as_str().unwrap_or("the observer returned nothing").trim().to_string(),
        cost: v["total_cost_usd"].as_f64().unwrap_or(0.0),
        ms: v["duration_ms"].as_u64().unwrap_or(t0.elapsed().as_millis() as u64),
    }
}

pub enum Phase {
    Prompt,
    Running { frame: usize, secs: u64 },
    Done(Answer),
    Idle,
}

pub struct View<'a> {
    pub pane: &'a Pane,
    pub q: &'a str,
    pub cursor: usize,
    pub phase: &'a Phase,
    pub th: &'a Theme,
}

pub fn render(f: &mut Frame, area: Rect, v: &View) {
    let th = v.th;
    let block = Block::bordered().border_style(Style::new().fg(th.meta)).title(Span::styled(
        format!(" ask · {} · {} ", v.pane.id, trunc(v.pane.label(), 28)),
        Style::new().fg(th.label),
    ));
    let inner = block.inner(area);
    let w = inner.width as usize;
    let mut lines = vec![Line::from(vec![
        Span::styled("ask> ", Style::new().fg(th.accent)),
        Span::styled(v.q.to_string(), Style::new().fg(th.value)),
    ])];
    lines.push(Line::from(""));
    match v.phase {
        Phase::Prompt => {}
        Phase::Idle => {
            lines.push(Line::from(Span::styled(
                "  this pane is idle — type your question into it directly",
                Style::new().fg(th.quote),
            )));
        }
        Phase::Running { frame, secs } => lines.push(Line::from(vec![
            Span::styled(format!("  {}  ", FRAMES[frame % 10]), Style::new().fg(th.accent)),
            Span::styled(format!("asking the observer …  {secs}s"), Style::new().fg(th.meta)),
        ])),
        Phase::Done(a) => {
            for l in wrap(&a.text, w.saturating_sub(4), 8) {
                lines.push(Line::from(vec![Span::raw("  "), Span::styled(l, Style::new().fg(th.value))]));
            }
        }
    }
    // footer pinned to the last row of the block
    while lines.len() + 1 < inner.height as usize {
        lines.push(Line::from(""));
    }
    let keys = match v.phase {
        Phase::Prompt => "↵ ask   esc close".to_string(),
        Phase::Idle => "↵ jump   esc close".to_string(),
        _ => "↵ close   a ask again   esc close".to_string(),
    };
    let left = match v.phase {
        Phase::Done(a) => format!("{}s · ${:.2} · haiku", a.ms / 1000, a.cost),
        _ => String::new(),
    };
    let pad = w.saturating_sub(left.chars().count() + keys.chars().count());
    lines.push(Line::from(vec![
        Span::styled(left, Style::new().fg(th.meta)),
        Span::raw(" ".repeat(pad)),
        Span::styled(keys, Style::new().fg(th.meta)),
    ]));
    f.render_widget(Paragraph::new(Text::from(lines)).block(block), area);
    if matches!(v.phase, Phase::Prompt) {
        f.set_cursor_position((inner.x + 5 + v.cursor as u16, inner.y));
    }
}

/// A line editor in twenty lines. tui-input 0.15 does support ratatui 0.30, but
/// one prompt does not earn a dependency.
fn edit(q: &mut String, cursor: &mut usize, code: KeyCode) {
    let chars: Vec<char> = q.chars().collect();
    match code {
        KeyCode::Char(c) => {
            let b = chars[..*cursor].iter().map(|c| c.len_utf8()).sum::<usize>();
            q.insert(b, c);
            *cursor += 1;
        }
        KeyCode::Backspace if *cursor > 0 => {
            let b = chars[..*cursor - 1].iter().map(|c| c.len_utf8()).sum::<usize>();
            q.remove(b);
            *cursor -= 1;
        }
        KeyCode::Delete if *cursor < chars.len() => {
            let b = chars[..*cursor].iter().map(|c| c.len_utf8()).sum::<usize>();
            q.remove(b);
        }
        KeyCode::Left => *cursor = cursor.saturating_sub(1),
        KeyCode::Right => *cursor = (*cursor + 1).min(chars.len()),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = chars.len(),
        _ => {}
    }
}

pub fn run(id: &str) -> Option<String> {
    let th = Theme::load();
    let p = crate::data::one_pane(id);
    let idle = p.state.is_empty() || p.state == "done" || p.sess.is_empty();
    let mut term = ratatui::init();
    let (mut q, mut cursor) = (String::new(), 0usize);
    let mut phase = if idle { Phase::Idle } else { Phase::Prompt };
    let mut rx: Option<(Receiver<Answer>, Instant)> = None;
    let mut action = None;
    let mut dirty = true;
    loop {
        if let Some((r, t0)) = &rx {
            match r.try_recv() {
                Ok(a) => {
                    phase = Phase::Done(a);
                    rx = None;
                    dirty = true;
                }
                Err(_) => {
                    let e = t0.elapsed();
                    phase = Phase::Running { frame: (e.as_millis() / 120) as usize, secs: e.as_secs() };
                    dirty = true;
                }
            }
        }
        if dirty {
            let v = View { pane: &p, q: &q, cursor, phase: &phase, th: &th };
            let _ = term.draw(|f| render(f, f.area(), &v));
            dirty = false;
        }
        if event::poll(Duration::from_millis(120)).unwrap_or(false)
            && let Ok(Event::Key(k)) = event::read()
            && k.kind == KeyEventKind::Press
        {
            dirty = true;
            match (&phase, k.code) {
                (_, KeyCode::Esc) => break,
                (Phase::Idle, KeyCode::Enter) => {
                    action = Some(format!("jump {}", p.id));
                    break;
                }
                (Phase::Prompt, KeyCode::Enter) => {
                    if q.trim().is_empty() {
                        break;
                    }
                    let (tx, r) = channel();
                    let (s, pa, qq) = (p.sess.clone(), p.path.clone(), q.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(observe(&s, &pa, &qq));
                    });
                    rx = Some((r, Instant::now()));
                    phase = Phase::Running { frame: 0, secs: 0 };
                }
                (Phase::Prompt, c) => edit(&mut q, &mut cursor, c),
                (Phase::Done(_), KeyCode::Char('a')) => {
                    q.clear();
                    cursor = 0;
                    phase = Phase::Prompt;
                }
                (Phase::Done(_), KeyCode::Enter) => break,
                _ => {}
            }
        }
    }
    ratatui::restore();
    action
}
