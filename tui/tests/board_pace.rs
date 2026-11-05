//! When the board's worker fetches and when it only captures (#492): a push
//! wake waits out PUSH_GAP after the last fetch, a new selection recaptures at
//! once and fetches nothing, and the last wake of a storm is always fetched.
//! No tmux and no sleep: every instant is an offset from one `Instant::now()`.
use cockpit_tui::board::{self, Ask, Job, Kind, PUSH_GAP, Pace, TICK, Turn, Want};
use ratatui::layout::Rect;
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

/// The worker against `asks` (ms, kind; each wants `top` = its index), a fetch
/// taking `fetch` ms and a capture 3 ms, until `until` ms: every job it runs,
/// (start ms, job, the top it captured). The first fetch starts at 0.
fn sim(asks: &[(u64, Kind)], fetch: u64, until: u64) -> Vec<(u64, Job, usize)> {
    let t = Instant::now();
    let at = |n: u64| t + ms(n);
    let (mut jobs, mut now, mut top) = (vec![(0, Job::Fetch, 0)], fetch, 0);
    let (mut pace, mut next) = (Pace::after(at(fetch)), 0);
    while now <= until {
        // what came during the last job, or by now, is read now
        while next < asks.len() && asks[next].0 <= now {
            top = pace.take(ask(asks[next].1, next), at(now)).top;
            next += 1;
        }
        if let Some(j) = pace.job(at(now)) {
            jobs.push((now, j, top));
            now += if j == Job::Fetch { fetch } else { 3 };
            if j == Job::Fetch {
                pace = Pace::after(at(now));
            }
            continue;
        }
        let due = pace.due().duration_since(t).as_millis() as u64;
        now = asks.get(next).map_or(due, |a| a.0.min(due));
    }
    jobs
}

fn every(step: u64, until: u64, kind: Kind) -> Vec<(u64, Kind)> {
    (1..=until / step).map(|k| (k * step, kind)).collect()
}

fn fetches(jobs: &[(u64, Job, usize)]) -> Vec<u64> {
    jobs.iter().filter(|j| j.1 == Job::Fetch).map(|j| j.0).collect()
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
fn every_held_selection_gets_its_capture_within_a_fetch() {
    let asks = every(33, 3000, Kind::Look);
    let jobs = sim(&asks, 40, 3000);
    for (k, (at, _)) in asks.iter().enumerate() {
        let seen = jobs.iter().any(|&(s, _, top)| s >= *at && s <= at + 40 && top >= k);
        assert!(seen, "selection {k} at {at} ms is not captured by {} ms", at + 40);
    }
}

#[test]
fn a_wait_whose_key_loop_is_gone_runs_nothing() {
    // (name, what was queued before the key loop went)
    let rows = [("a push", Kind::Soon), ("a selection", Kind::Look), ("refresh", Kind::Now)];
    for (name, kind) in rows {
        let (tx, rx) = mpsc::channel();
        tx.send(ask(kind, 0)).unwrap();
        drop(tx);
        let mut w = want(0);
        let job = board::wait(&rx, &mut Pace::after(Instant::now()), &mut w);
        assert_eq!(job, None, "{name}");
    }
}

#[test]
fn a_wait_past_its_gap_fetches_the_pushed_want() {
    let (tx, rx) = mpsc::channel();
    tx.send(ask(Kind::Soon, 3)).unwrap();
    let mut p = Pace::after(Instant::now().checked_sub(TICK / 2).unwrap());
    let mut w = want(0);
    assert_eq!(board::wait(&rx, &mut p, &mut w), Some(Job::Fetch));
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
