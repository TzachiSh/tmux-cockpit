//! A sample for the code viewer's goldens: comments, strings, numbers,
//! keywords, a tab, a wide character and a line too long for 40 columns.
use std::collections::HashMap;

/// Counts each word, ignoring case. "# not a heading", *not emphasis*.
pub fn count(text: &str) -> HashMap<String, usize> {
	let mut out = HashMap::new();
	for w in text.split_whitespace() {
		*out.entry(w.to_lowercase()).or_insert(0) += 1;
	}
	out
}

fn main() {
    let words = count("the cat and the hat — 猫 and 帽子, a line that runs well past forty columns");
    let n = 0x2a + 1_000;
    /* a block
       comment */
    println!("{} words, {n}", words.len());
}
