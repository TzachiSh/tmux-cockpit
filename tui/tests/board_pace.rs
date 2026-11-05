//! When the board's worker fetches and when it only captures (#492): a push
//! wake waits out PUSH_GAP after the last fetch, a new selection recaptures at
//! once and fetches nothing, and the last wake of a storm is always fetched.
//! No tmux and no sleep: every instant is an offset from one `Instant::now()`.
use cockpit_tui::board::{self, Ask, Gone, Inbox, Job, Kind, PUSH_GAP, Pace, Reads, Sched, Snapshot, TICK, Turn, Want};
use ratatui::layout::Rect;
use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn want(top: usize) -> Want {
    Want {
        area: Rect::new(0, 0, 100, 30),
        sel: 0,
        top,
        tiles: true,
        pick: None,
    }
}

fn ask(kind: Kind, top: usize) -> Ask {
    Ask { kind, want: want(top) }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

const GAP: u64 = PUSH_GAP.as_millis() as u64;

/// When the fetch is due, as an offset from `t`, so a failure prints ms.
fn due(p: &Pace, t: Instant) -> Duration {
    p.due().duration_since(t)
}

#[test]
fn with_no_ask_the_next_fetch_is_the_one_second_tick() {
    let t = Instant::now();
    assert_eq!(due(&Pace::after(t), t), TICK);
}

#[test]
fn a_push_inside_the_gap_waits_for_its_end() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Soon, 0), t + ms(10));
    assert_eq!(due(&p, t), PUSH_GAP);
}

#[test]
fn a_push_after_a_quiet_gap_fetches_at_once() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Soon, 0), t + PUSH_GAP + ms(150));
    assert_eq!(due(&p, t), PUSH_GAP + ms(150));
}

#[test]
fn the_refresh_key_inside_the_gap_fetches_at_once() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Soon, 0), t + ms(5));
    p.take(ask(Kind::Now, 0), t + ms(10));
    assert_eq!(due(&p, t), ms(10));
}

#[test]
fn a_push_after_the_refresh_key_does_not_put_it_off() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Now, 0), t + ms(10));
    p.take(ask(Kind::Soon, 0), t + ms(11));
    assert_eq!(due(&p, t), ms(10));
}

#[test]
fn a_new_selection_captures_at_once_and_fetches_nothing() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Look, 0), t + ms(10));
    assert_eq!(p.job(t + ms(10)), Some(Job::Capture));
    assert_eq!(p.job(t + ms(10)), None, "one capture per ask");
    assert_eq!(due(&p, t), TICK, "the fetch stays on the tick");
}

#[test]
fn a_capture_keeps_a_pending_push_where_it_was() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(ask(Kind::Soon, 0), t + ms(10));
    p.take(ask(Kind::Look, 0), t + ms(20));
    assert_eq!(p.job(t + ms(20)), Some(Job::Capture));
    assert_eq!(p.job(t + PUSH_GAP - ms(1)), None);
    assert_eq!(p.job(t + PUSH_GAP), Some(Job::Fetch));
}

#[test]
fn the_newest_ask_names_what_to_capture() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    assert_eq!(p.take(ask(Kind::Soon, 7), t + ms(10)).top, 7);
    assert_eq!(p.take(ask(Kind::Look, 9), t + ms(20)).top, 9);
}

/// The worker's inbox on a fake clock: `asks` (ms, kind) come at their
/// times, ask k wanting `top` = k + 1. The clock is shared with the job, which
/// moves it on by what the job costs.
struct Timeline {
    t: Instant,
    clock: Rc<Cell<u64>>,
    asks: Vec<(u64, Kind)>,
    next: usize,
}

impl Timeline {
    fn pop(&mut self) -> Option<Ask> {
        let (at, kind) = *self.asks.get(self.next)?;
        self.clock.set(self.clock.get().max(at));
        self.next += 1;
        Some(ask(kind, self.next))
    }
}

impl Inbox for Timeline {
    fn now(&self) -> Instant {
        self.t + ms(self.clock.get())
    }
    fn ready(&mut self) -> Result<Option<Ask>, Gone> {
        let due = self.asks.get(self.next).is_some_and(|a| a.0 <= self.clock.get());
        Ok(if due { self.pop() } else { None })
    }
    fn until(&mut self, until: Instant) -> Result<Option<Ask>, Gone> {
        let u = until.saturating_duration_since(self.t).as_millis() as u64;
        if self.asks.get(self.next).is_some_and(|a| a.0 <= u) {
            return Ok(self.pop());
        }
        self.clock.set(self.clock.get().max(u));
        Ok(None)
    }
}

/// The board's own worker loop, [`board::work`], against `asks`, a fetch
/// taking `fetch` ms and a capture 3 ms, until `until` ms: every job it ran,
/// (start ms, it fetched, the top it captured). The first fetch starts at 0.
fn sim(asks: &[(u64, Kind)], fetch: u64, until: u64) -> Vec<(u64, bool, usize)> {
    let clock = Rc::new(Cell::new(0));
    let (c, mut jobs) = (clock.clone(), vec![]);
    let mut inbox = Timeline {
        t: Instant::now(),
        clock,
        asks: asks.to_vec(),
        next: 0,
    };
    board::work(&mut inbox, want(0), |fetches, w| {
        if c.get() > until {
            return false;
        }
        jobs.push((c.get(), fetches, w.top));
        c.set(c.get() + if fetches { fetch } else { 3 });
        true
    });
    jobs
}

fn every(step: u64, until: u64, kind: Kind) -> Vec<(u64, Kind)> {
    (1..=until / step).map(|k| (k * step, kind)).collect()
}

fn fetches(jobs: &[(u64, bool, usize)]) -> Vec<u64> {
    jobs.iter().filter(|j| j.1).map(|j| j.0).collect()
}

#[test]
fn a_storm_fetches_at_most_once_per_gap() {
    // 100 wakes a second for 3 s, a 40 ms fetch: at most one fetch per 290 ms
    let f = fetches(&sim(&every(10, 3000, Kind::Soon), 40, 3000));
    let room = 3000 / (GAP + 40) + 2;
    assert!(f.len() as u64 <= room, "{} fetches, room for {room}: {f:?}", f.len());
}

#[test]
fn a_steady_storm_still_refreshes_every_gap() {
    let f = fetches(&sim(&every(10, 3000, Kind::Soon), 40, 3000));
    let most = f.windows(2).map(|w| w[1] - w[0]).max().unwrap();
    assert!(most <= GAP + 40, "a {most} ms gap between fetches: {f:?}");
}

#[test]
fn the_last_wake_of_a_storm_is_fetched() {
    // the wakes stop at 1005 ms, mid-gap: a fetch must still follow the last
    let f = fetches(&sim(&every(15, 1005, Kind::Soon), 40, 1600));
    let after = f.iter().find(|&&s| s >= 1005);
    assert!(after.is_some_and(|&s| s <= 1005 + GAP + 40), "no fetch soon after 1005 ms: {f:?}");
}

#[test]
fn a_held_arrow_key_fetches_on_the_tick_only() {
    // j held at 30 Hz for 3 s: 90 selections
    let f = fetches(&sim(&every(33, 3000, Kind::Look), 40, 3000));
    assert!(f.len() <= 4 * 3, "{} full fetches in 3 s: {f:?}", f.len());
}

#[test]
fn a_held_arrow_key_still_fetches_every_tick() {
    // a capture must not push the tick out, or held j never reads tmux
    let f = fetches(&sim(&every(33, 3000, Kind::Look), 40, 3000));
    let most = f.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(u64::MAX);
    assert!(most <= TICK.as_millis() as u64 + 40 + 3, "a {most} ms gap between fetches: {f:?}");
}

#[test]
fn every_held_selection_gets_its_capture_within_a_fetch() {
    let asks = every(33, 3000, Kind::Look);
    let jobs = sim(&asks, 40, 3000);
    for (k, (at, _)) in asks.iter().enumerate() {
        let seen = jobs.iter().any(|&(s, _, top)| s >= *at && s <= at + 40 && top > k);
        assert!(seen, "selection {k} at {at} ms is not captured by {} ms", at + 40);
    }
}

#[test]
fn a_wait_whose_key_loop_is_gone_runs_nothing() {
    // (name, what was queued before the key loop went)
    let rows = [("a push", Kind::Soon), ("a selection", Kind::Look), ("refresh", Kind::Now)];
    for (name, kind) in rows {
        let (tx, mut rx) = mpsc::channel();
        tx.send(ask(kind, 0)).unwrap();
        drop(tx);
        let mut w = want(0);
        assert!(!board::wait(&mut rx, &mut Sched::start(Instant::now()), &mut w), "{name}");
    }
}

#[test]
fn a_wait_past_its_gap_fetches_the_pushed_want() {
    let (tx, mut rx) = mpsc::channel();
    tx.send(ask(Kind::Soon, 3)).unwrap();
    let mut s = Sched::start(Instant::now().checked_sub(TICK / 2).unwrap());
    s.ran(Instant::now().checked_sub(TICK / 2).unwrap());
    let mut w = want(0);
    assert!(board::wait(&mut rx, &mut s, &mut w));
    assert!(s.fetches());
    assert_eq!(w.top, 3);
    drop(tx);
}

#[test]
fn a_push_and_a_resize_ask_by_what_they_are() {
    let r = |w, h| Rect::new(0, 0, w, h);
    let (s, wide) = (Some(r(100, 30)), Some(r(120, 30)));
    // (name, a push came, the resize event, what is asked, redrawn)
    let rows = [
        ("a push and its own SIGWINCH", true, s, vec![Kind::Soon], true),
        ("a SIGWINCH of the same size", false, s, vec![], true),
        ("a new size", false, wide, vec![Kind::Look], true),
        ("a push during a new size", true, wide, vec![Kind::Soon, Kind::Look], true),
        ("a push alone", true, None, vec![Kind::Soon], false),
        ("nothing", false, None, vec![], false),
    ];
    for (name, pushed, resize, asks, redraw) in rows {
        let mut sized = r(100, 30);
        assert_eq!(board::turn(pushed, resize, &mut sized), Turn { asks, redraw }, "{name}");
    }
}

#[test]
fn a_push_behind_an_ignored_sigwinch_is_still_fetched_at_the_gap_end() {
    let t = Instant::now();
    let mut sized = Rect::new(0, 0, 100, 30);
    let mut p = Pace::after(t);
    for k in board::turn(true, Some(sized), &mut sized).asks {
        p.take(ask(k, 0), t + ms(10));
    }
    assert_eq!(p.job(t + PUSH_GAP - ms(1)), None);
    assert_eq!(p.job(t + PUSH_GAP), Some(Job::Fetch));
}

#[test]
fn a_new_size_recaptures_at_once() {
    let t = Instant::now();
    let mut sized = Rect::new(0, 0, 100, 30);
    let mut p = Pace::after(t);
    for k in board::turn(false, Some(Rect::new(0, 0, 80, 30)), &mut sized).asks {
        p.take(ask(k, 0), t + ms(10));
    }
    assert_eq!(p.job(t + ms(10)), Some(Job::Capture));
}

#[test]
fn a_new_size_then_its_wakes_recapture_once() {
    let (mut sized, wide) = (Rect::new(0, 0, 100, 30), Rect::new(0, 0, 120, 30));
    let asks: Vec<_> = (0..4).map(|_| board::turn(true, Some(wide), &mut sized).asks).collect();
    assert_eq!(asks[0], vec![Kind::Soon, Kind::Look], "the resize itself");
    assert_eq!(asks[1..], [vec![Kind::Soon], vec![Kind::Soon], vec![Kind::Soon]], "its wakes");
}

fn snap(fresh: bool, ok: bool, took: u64) -> Snapshot {
    let (panes, pvs, sessions) = (vec![], vec![], BTreeSet::new());
    Snapshot {
        panes,
        pvs,
        took: ms(took),
        ok,
        sessions,
        fresh,
    }
}

#[test]
fn a_failed_fetch_and_two_selections_are_one_failure() {
    let t = Instant::now();
    let mut reads = Reads::new(t);
    board::drain(snap(true, true, 40), std::iter::empty(), &mut reads, t);
    board::drain(snap(true, false, 40), std::iter::empty(), &mut reads, t + ms(10));
    board::drain(snap(false, false, 3), std::iter::empty(), &mut reads, t + ms(20));
    board::drain(snap(false, false, 3), std::iter::empty(), &mut reads, t + ms(30));
    assert_eq!(reads.fails, 1);
    assert!(!reads.lost(t + ms(30)), "no lost card after one failed read");
}

#[test]
fn a_capture_drained_with_a_slow_fetch_does_not_hide_it() {
    let t = Instant::now();
    let rest = [snap(false, true, 3)].into_iter();
    let d = board::drain(snap(true, true, 900), rest, &mut Reads::new(t), t);
    assert_eq!(d.fetched, Some(ms(900)));
    assert!(!d.snap.fresh, "the newest snapshot is the one drawn");
}
