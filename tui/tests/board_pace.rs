//! When the board's worker fetches (#492): a push wake waits out PUSH_GAP after
//! the last fetch, a key or a resize does not, and the last wake of a storm is
//! always followed by a fetch.
//! No tmux and no sleep: every instant is an offset from one `Instant::now()`.
use cockpit_tui::board::{self, Ask, PUSH_GAP, Pace, TICK, Want};
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

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

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
    p.take(Ask::Push(want(0)), t + ms(10));
    assert_eq!(due(&p, t), PUSH_GAP);
}

#[test]
fn a_push_after_a_quiet_gap_fetches_at_once() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(Ask::Push(want(0)), t + PUSH_GAP + ms(150));
    assert_eq!(due(&p, t), PUSH_GAP + ms(150));
}

#[test]
fn a_key_inside_the_gap_fetches_at_once() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(Ask::Push(want(0)), t + ms(5));
    p.take(Ask::Now(want(0)), t + ms(10));
    assert_eq!(due(&p, t), ms(10));
}

#[test]
fn a_push_after_a_key_does_not_put_the_key_off() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    p.take(Ask::Now(want(0)), t + ms(10));
    p.take(Ask::Push(want(0)), t + ms(11));
    assert_eq!(due(&p, t), ms(10));
}

#[test]
fn the_newest_ask_names_what_to_capture() {
    let t = Instant::now();
    let mut p = Pace::after(t);
    let w = p.take(Ask::Push(want(7)), t + ms(10));
    assert_eq!(w.top, 7);
    assert_eq!(p.take(Ask::Now(want(9)), t + ms(20)).top, 9);
}

/// The worker against a push every `every` ms until `until` ms, each fetch
/// taking `cost` ms: the start of every fetch in ms, the first at 0.
fn storm(every: u64, until: u64, cost: u64) -> Vec<u64> {
    let t = Instant::now();
    let at = |n: u64| t + ms(n);
    let (mut starts, mut next_push, mut start) = (vec![0], every, 0u64);
    while start <= until + TICK.as_millis() as u64 {
        let done = start + cost;
        let mut p = Pace::after(at(done));
        // an ask that came during the fetch is read once it ends
        while next_push <= until && at(next_push) <= p.due().max(at(done)) {
            p.take(Ask::Push(want(0)), at(next_push.max(done)));
            next_push += every;
        }
        start = p.due().duration_since(t).as_millis() as u64;
        if next_push > until && start > until + PUSH_GAP.as_millis() as u64 + cost {
            break;
        }
        starts.push(start);
    }
    starts
}

#[test]
fn a_storm_fetches_at_most_once_per_gap() {
    // 100 wakes a second for 3 s, a 40 ms fetch: at most one fetch per 290 ms
    let starts = storm(10, 3000, 40);
    let room = 3000 / (PUSH_GAP.as_millis() as u64 + 40) + 3;
    assert!(starts.len() as u64 <= room, "{} fetches, room for {room}: {starts:?}", starts.len());
}

#[test]
fn a_steady_storm_still_refreshes_every_gap() {
    let starts = storm(10, 3000, 40);
    let most = starts.windows(2).map(|w| w[1] - w[0]).max().unwrap();
    assert!(
        most <= PUSH_GAP.as_millis() as u64 + 40,
        "a {most} ms gap between fetches: {starts:?}"
    );
}

#[test]
fn the_last_wake_of_a_storm_is_fetched() {
    // the wakes stop at 1005 ms, mid-gap: one fetch must start after the last
    let starts = storm(15, 1005, 40);
    let last = *starts.last().unwrap();
    assert!(last >= 1005, "the last fetch starts at {last} ms: {starts:?}");
    assert!(
        last <= 1005 + PUSH_GAP.as_millis() as u64 + 40,
        "the trailing fetch waits {last} ms"
    );
}

#[test]
fn a_wait_whose_key_loop_is_gone_never_fetches() {
    let (tx, rx) = mpsc::channel();
    tx.send(Ask::Push(want(0))).unwrap();
    drop(tx);
    assert!(board::wait(&rx, Instant::now(), want(0)).is_none());
}

#[test]
fn a_wait_past_its_gap_returns_the_pushed_want() {
    let (tx, rx) = mpsc::channel();
    tx.send(Ask::Push(want(3))).unwrap();
    let long_ago = Instant::now().checked_sub(TICK * 10).unwrap();
    assert_eq!(board::wait(&rx, long_ago, want(0)).map(|w| w.top), Some(3));
    drop(tx);
}

#[test]
fn a_resize_asks_only_when_the_size_moved() {
    let r = |w, h| Rect::new(0, 0, w, h);
    // (name, last asked size, the size now, asks)
    let rows = [
        ("the push's own SIGWINCH", r(100, 30), r(100, 30), false),
        ("a wider terminal", r(100, 30), r(120, 30), true),
        ("a shorter terminal", r(100, 30), r(100, 20), true),
    ];
    for (name, before, now, asks) in rows {
        let mut last = before;
        assert_eq!(board::resized(&mut last, now), asks, "{name}");
        assert_eq!(last, now, "{name}: the size is remembered");
    }
}
