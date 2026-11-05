//! What a code file's rows cost (#365 review): bytes per row, and the peak of a
//! relayout. Its own binary, for its counting allocator; one test, so no other
//! test allocates while it measures.
use cockpit_tui::mdview::render::Opts;
use cockpit_tui::mdview::{Mode, Screen};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = NOW.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(n, Relaxed);
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        NOW.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Bytes held now, and the peak since the last call.
fn mark() -> (usize, usize) {
    let now = NOW.load(Relaxed);
    (now, PEAK.swap(now, Relaxed))
}

/// 1 MB of empty lines: the most rows a file under the cap can make.
#[test]
fn code_rows_are_small_and_a_relayout_frees_the_old_ones() {
    let rows = 1 << 20;
    let mut s = Screen::new("x.txt", Ok("\n".repeat(rows)), Opts::default()).with_mode(Mode::Code(None));
    let (before, _) = mark();
    s.layout(76);
    let (held, _) = mark();
    assert_eq!(s.rows().len(), rows);
    let per_row = (held - before) / rows;
    s.layout(116);
    let (_, peak) = mark();
    let extra = peak - held;
    assert!(
        extra < (held - before) / 2,
        "a relayout peaked {extra} bytes over the {} its rows hold",
        held - before
    );
    assert!(per_row <= 80, "{per_row} bytes a row");
    drop(s);

    // a cell's vector holds its one cell, not the four a push leaves room for
    let rows = (1 << 20) / 3;
    let mut s = Screen::new("x.txt", Ok("ab\n".repeat(rows)), Opts::default()).with_mode(Mode::Code(None));
    let (before, _) = mark();
    s.layout(76);
    let (held, _) = mark();
    let per_row = (held - before) / rows;
    // measured 112 (x86_64, glibc): the 8 over it are room for another allocator's
    // rounding; an unshrunk cell vector is 208, so the bound still holds the line
    assert!(per_row <= 120, "{per_row} bytes an `ab` row");
}
