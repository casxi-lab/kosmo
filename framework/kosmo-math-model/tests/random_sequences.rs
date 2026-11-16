//! Random edit sequences from a fixed seed. After every step nothing has
//! panicked, the line holds only characters of the character set and at
//! most `MAX_ITEMS` items, the cursor and the selection are valid, a
//! rejected edit has changed nothing, and the source version has increased
//! by exactly 1 if and only if the content changed.
//!
//! The generator reports its seed and how many values it had drawn when a
//! check fails, which identifies the failing sequence and step.

mod support;

use std::collections::BTreeMap;

use kosmo_math_model::{Completeness, EditBuffer, Fragment, MAX_ITEMS, Rejected, Snapshot};
use support::{line, text};

/// The seed of the first sequence; sequence `n` uses `SEED + n`.
const SEED: u64 = 0x6b6f_736d_6f00_0001;

/// The number of sequences.
const SEQUENCES: u64 = 1_000;

/// The most operations in one sequence.
const MAX_STEPS: u64 = 64;

/// The character set, written out apart from the library.
const CHARACTER_SET: &str =
    "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_.+-*/^() ";

/// Characters outside the set that the sequences also try to insert.
const OUTSIDE: [char; 8] = [',', '=', '\t', '\n', '□', 'λ', '\u{0}', '\u{a0}'];

/// The SplitMix64 generator. When a test panics while a generator is alive,
/// it reports its seed and how many values it produced.
struct SplitMix64 {
    seed: u64,
    state: u64,
    drawn: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> SplitMix64 {
        SplitMix64 {
            seed,
            state: seed,
            drawn: 0,
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        self.drawn += 1;
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A value in `0..bound`, up to the generator's bias; 0 when `bound` is 0.
    fn below(&mut self, bound: usize) -> usize {
        let bound = bound as u64;
        if bound == 0 {
            0
        } else {
            (self.next() % bound) as usize
        }
    }

    /// Whether a one-in-`n` chance came up.
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }

    fn character(&mut self) -> char {
        if self.one_in(8) {
            OUTSIDE[self.below(OUTSIDE.len())]
        } else {
            let set: Vec<char> = CHARACTER_SET.chars().collect();
            set[self.below(set.len())]
        }
    }

    /// Text to insert: mostly a few characters of the set, sometimes a long
    /// run of brackets, minus signs or digits that comes near the depth
    /// limit, rarely text near the byte limit, and sometimes a character
    /// outside the set.
    fn text(&mut self) -> String {
        let mut text: String = if self.one_in(16) {
            let runs = ['(', '-', '1', '+'];
            let c = runs[self.below(runs.len())];
            std::iter::repeat_n(c, 1 + self.below(80)).collect()
        } else if self.one_in(64) {
            let set: Vec<char> = CHARACTER_SET.chars().collect();
            let length = 4_000 + self.below(200);
            (0..length).map(|_| set[self.below(set.len())]).collect()
        } else {
            let set: Vec<char> = CHARACTER_SET.chars().collect();
            let length = self.below(9);
            (0..length).map(|_| set[self.below(set.len())]).collect()
        };
        if self.one_in(16) {
            let mut chars: Vec<char> = text.chars().collect();
            let at = self.below(chars.len() + 1);
            chars.insert(at, OUTSIDE[self.below(OUTSIDE.len())]);
            text = chars.into_iter().collect();
        }
        text
    }
}

impl Drop for SplitMix64 {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "random edit sequence: seed {:#x}, failure after {} values were drawn",
                self.seed, self.drawn
            );
        }
    }
}

/// Applies one of the thirteen editing operations, drawn at random with
/// random arguments. Snapshots for fragment insertion come from `pool`.
fn apply(
    buffer: &mut EditBuffer,
    rng: &mut SplitMix64,
    pool: &[(Snapshot, String)],
) -> Result<(), Rejected> {
    match rng.below(13) {
        0 => buffer.insert_char(rng.character()),
        1 => buffer.insert_text(&rng.text()),
        2 => buffer.delete_backward(),
        3 => buffer.delete_forward(),
        4 => {
            buffer.move_left();
            Ok(())
        }
        5 => {
            buffer.move_right();
            Ok(())
        }
        6 => {
            buffer.move_to_start();
            Ok(())
        }
        7 => {
            buffer.move_to_end();
            Ok(())
        }
        8 => {
            buffer.extend_left();
            Ok(())
        }
        9 => {
            buffer.extend_right();
            Ok(())
        }
        10 => {
            buffer.select_all();
            Ok(())
        }
        11 => buffer.clear(),
        _ => match pool.get(rng.below(pool.len())) {
            Some((snapshot, _)) => {
                // Ranges up to two items past the end, so that some fragments
                // do not lie in the snapshot.
                let bound = snapshot.items().len() + 3;
                let (one, other) = (rng.below(bound), rng.below(bound));
                match Fragment::new(snapshot.root(), one.min(other)..one.max(other)) {
                    Some(fragment) => buffer.insert_fragment(snapshot, fragment),
                    None => Ok(()),
                }
            }
            None => Ok(()),
        },
    }
}

/// Checks the invariants after one step that turned `before` into `after`
/// with `outcome`.
fn check_step(before: &EditBuffer, after: &EditBuffer, outcome: Result<(), Rejected>) {
    let items = text(after.items());
    assert!(
        items.chars().all(|c| CHARACTER_SET.contains(c)),
        "{items:?}"
    );
    assert!(after.items().len() <= MAX_ITEMS);
    assert_eq!(after.root(), before.root());
    let cursor = after.cursor();
    assert_eq!(cursor.block, after.root());
    assert!(cursor.index <= after.items().len());
    if let Some(selection) = after.selection() {
        assert_eq!(selection.anchor().block, after.root());
        assert_eq!(selection.active(), cursor);
        assert_ne!(selection.anchor(), selection.active());
        assert!(selection.anchor().index <= after.items().len());
    }
    if outcome.is_err() {
        assert_eq!(after, before, "{outcome:?}");
    }
    let changed = after.items() != before.items();
    let expected = before.version().get() + u64::from(changed);
    assert_eq!(after.version().get(), expected, "{outcome:?}");
    match after.completeness() {
        Completeness::Complete => {}
        Completeness::Incomplete(holes) => {
            assert!(!holes.is_empty());
            assert!(
                holes
                    .windows(2)
                    .all(|w| w[0].position.index <= w[1].position.index)
            );
            assert!(holes.iter().all(|hole| {
                hole.position.block == after.root() && hole.position.index <= after.items().len()
            }));
        }
        Completeness::Invalid(invalid) => {
            assert_eq!(invalid.fragment.block(), after.root());
            assert!(!invalid.fragment.is_empty());
            assert!(invalid.fragment.end() <= after.items().len());
        }
    }
}

#[test]
fn random_edit_sequences_keep_the_invariants() -> Result<(), Rejected> {
    let other_line = line("x+1")?.snapshot();
    // How often each outcome came up, so that a change of the generator
    // that stops reaching a rejection fails.
    let mut outcomes: BTreeMap<String, u64> = BTreeMap::new();
    for sequence in 0..SEQUENCES {
        let mut rng = SplitMix64::new(SEED.wrapping_add(sequence));
        let mut buffer = EditBuffer::new();
        let mut pool = vec![(other_line.clone(), "x+1".to_string())];
        let steps = 1 + rng.below(MAX_STEPS as usize);
        for _ in 0..steps {
            let before = buffer.clone();
            let outcome = apply(&mut buffer, &mut rng, &pool);
            check_step(&before, &buffer, outcome);
            let name = match outcome {
                Ok(()) if buffer.items() != before.items() => "changed".to_string(),
                Ok(()) => "unchanged".to_string(),
                Err(Rejected::Character(_)) => "Character".to_string(),
                Err(rejected) => format!("{rejected:?}"),
            };
            *outcomes.entry(name).or_default() += 1;
            if rng.one_in(4) {
                if pool.len() > 4 {
                    pool.remove(1);
                }
                pool.push((buffer.snapshot(), text(buffer.items())));
            }
        }
        // Snapshots keep the content they were taken with. Versions are
        // compared only within one buffer, so the other line's is skipped.
        for (snapshot, taken) in &pool {
            assert_eq!(&text(snapshot.items()), taken);
        }
        for (snapshot, _) in pool.iter().skip(1) {
            assert!(snapshot.version() <= buffer.version());
        }
    }
    for name in [
        "changed",
        "unchanged",
        "Character",
        "InsertTooLong",
        "TooManyItems",
        "TooDeep",
        "NotInSnapshot",
    ] {
        assert!(outcomes.contains_key(name), "{name}: {outcomes:?}");
    }
    Ok(())
}
