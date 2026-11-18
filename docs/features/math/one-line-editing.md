# One-line editing

The library `kosmo-math-model` in the `framework/` workspace holds a formula
while you edit it on one line: the characters you typed, the cursor, the
selection and the source version. It keeps incomplete input such as `1/□`.
Holes are derived from the line, never stored, and the result of a
computation never changes the line you are editing.

The library does not depend on kalyx. It reads a line with the grammar of
kalyx 0.1.0 to find its holes and to tell whether it is complete. The kalyx
adapter crate `kosmo-math-kalyx` turns a line into the text that kalyx reads,
as [formula conversion](formula-conversion.md) describes; the display model
of results is planned for this library and that crate, the result view for
the math controls crate `kosmo-math-controls`, and two-dimensional templates
such as fractions for this library. The design rules that it follows are in the [editing
structure](../../architecture.md#editing-structure) and
[versions](../../architecture.md#versions) sections of the architecture.

## Using it

An `EditBuffer` is one line. Its operations edit at the cursor and return
`Result<(), Rejected>`; cursor and selection commands cannot fail. This is
the same example as the crate documentation, whose copy `cargo test` runs:

```rust
use kosmo_math_model::{Completeness, EditBuffer, HoleKind, Item, Rejected};

fn main() -> Result<(), Rejected> {
    // `2*` misses its second operand, at index 2.
    let mut line = EditBuffer::new();
    line.insert_text("2*")?;
    let holes = match line.completeness() {
        Completeness::Incomplete(holes) => holes,
        _ => Vec::new(),
    };
    assert_eq!(holes.len(), 1);
    assert_eq!(holes[0].kind, HoleKind::MissingOperand);
    assert_eq!(holes[0].position.index, 2);

    // Copying the line `x+1` into the hole adds brackets.
    let mut other = EditBuffer::new();
    other.insert_text("x+1")?;
    let snapshot = other.snapshot();
    line.insert_fragment(&snapshot, snapshot.line())?;
    let expected: Vec<Item> = "2*(x+1)".chars().map(Item::Char).collect();
    assert_eq!(line.items(), expected.as_slice());
    assert_eq!(line.completeness(), Completeness::Complete);

    // Two edits changed the content, so the source version is 2.
    assert_eq!(line.version().get(), 2);
    Ok(())
}
```

Run the library's tests from the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-model
```

## The line

- A line is the root block of the editing structure, a sequence of items.
  One-line input has only characters; `Item` and `TemplateKind` are
  `#[non_exhaustive]`, and `TemplateKind` has no variant yet, so no template
  can be built.
- The character set has 72 characters: the digits, the ASCII letters, `_`,
  `.`, `+`, `-`, `*`, `/`, `^`, `(`, `)` and the space. Every character is
  one item and one byte. Spaces are kept as typed and only separate tokens.
- The root block has the identity `NodeId` 0 for the life of the buffer.
  Identities are handed out in increasing order and never reused; clearing
  the line keeps the root and does not reset the counter. Characters have no
  identity.
- A `Position` is a block and an index between its items, from 0 to the
  number of items. A `Fragment` is a block and a half-open item range. A
  `Selection` is an anchor and an active end in the same block, never equal;
  the cursor is the active end, and `Selection::fragment` gives the selected
  items in order.

## Editing operations

| Operation | What it does |
| --- | --- |
| `insert_char(c)` | Inserts `c` at the cursor, replacing the selection, and puts the cursor after it |
| `insert_text(text)` | Inserts the whole text or nothing, replacing the selection |
| `delete_backward()` | Deletes the selection, or else the item before the cursor; at the start of the line without a selection it changes nothing |
| `delete_forward()` | Deletes the selection, or else the item after the cursor; at the end of the line without a selection it changes nothing |
| `move_left()`, `move_right()` | Move the cursor one item; with a selection, to its start or end instead, ending the selection |
| `move_to_start()`, `move_to_end()` | Move the cursor to the start or end of the line and end the selection |
| `extend_left()`, `extend_right()` | Move the active end one item, starting a selection at the cursor when there is none; at the line's end in that direction nothing changes, and coming back to the anchor ends the selection |
| `select_all()` | Puts the anchor at the start of the line and the cursor at its end; an empty line has no selection |
| `clear()` | Empties the line and puts the cursor at its start |
| `insert_fragment(snapshot, fragment)` | Inserts a copy of a fragment of a snapshot, replacing the selection, in brackets where it needs them |

Every edit checks the character set and the limits before it changes
anything. A rejected edit returns the reason and changes nothing: not the
items, the cursor, the selection or the source version.

`insert_fragment` puts the copy in brackets when the fragment is not a single
operand (a number, a name, a call or a bracketed expression) and an operand is
missing where it goes, so that the copy stays one operand: the line `x+1`
inserted after `2*` gives `2*(x+1)`, and before `*2` gives `(x+1)*2`. A copy
that goes where no operand is missing, and a fragment without a token, are
inserted as they are. A fragment whose brackets do not pair is put in brackets
too, but the added brackets can then pair with brackets of the copy or of the
line, so the copy need not stay one operand: `(1` inserted after `2*` gives
the incomplete `2*((1)`, and `x)*2` inserted after `(3-` gives `(3-(x)*2)`,
which is complete and means $3 - 2x$, while typing the same characters gives
`(3-x)*2`, which means $(3 - x) \cdot 2$. The fragment must come from the
snapshot it is used with, such as its `line()` or the range of an error about
it: only a range outside the snapshot is detected, and such a fragment is
rejected.

## Holes and completeness

`completeness()` reads the line and gives exactly one of `Complete`,
`Incomplete` with its holes in reading order, or `Invalid` with the first
invalid part.

The tokens are numbers, names, the operators `+`, `-`, `*`, `/` and `^`, and
brackets. A number is a run of digits and points; a name is a letter or `_`
followed by letters, digits and `_`. A number ends where a letter or `_`
starts, so `2x` is the number `2` followed by the name `x`.
`Snapshot::tokens()` gives the tokens of a line in reading order, each with
its kind, `Number`, `Name`, `Operator`, `LeftBracket` or `RightBracket`, and
its items. Spaces belong to no token, and a run of digits and points that
forms no number, such as `1.2.3`, is a number token of an invalid line.

A hole sits at the start of the token after it, or at the end of the line
when no token follows:

| Hole | Meaning | Examples, with the hole's index |
| --- | --- | --- |
| `EmptyLine` | The line has no token | the empty line (0); a line of three spaces (3) |
| `MissingOperand` | An operand is missing | `1/` (2); `+` (0 and 1); `x^` (2); `-` (1); `1+-` (3); `()` (1) |
| `MissingOperator` | Two operands follow each other | `2x` (1); `2(x+1)` (1); `(x)(y)` (3) |
| `MissingRightBracket` | A left bracket is not closed; one hole for each | `(` (1, after its missing operand); `(1+2` (4); `((1)` (4) |

| Completeness | When | Examples |
| --- | --- | --- |
| `Complete` | The line follows the grammar | `1+2*3`, `-2^2`, `2^-2`, `--2`, `.5`, `1.`, `f(x)`, `f (x)`, `pi` |
| `Incomplete` | Something is missing and no part is invalid | `1/`, `2x`, `(1+2`, `+1`, `1++2`, `f()` |
| `Invalid` | A run of digits and points that forms no number, or a right bracket without a left one before it | `1.2.3`, `.`, `1)`, `)(` |

- `.5` and `1.` are complete. kalyx 0.1.0 does not read them, so
  [formula conversion](formula-conversion.md) writes them as `0.5` and `1`.
- A call, a name followed by a bracketed expression with spaces allowed
  before the bracket, is an operand, and so are the reserved names `pi`, `i`
  and `undefined`. They are complete here; kalyx 0.1.0 reports them as
  unsupported. The brackets of a call hold an expression like any other, so
  `f()` misses an operand and `f(` a right bracket.
- kalyx 0.1.0 has no juxtaposition, no unary plus and no commas: `2x` and
  `+1` miss something, and `,` cannot be typed.
- Filling each hole with what it misses, an operand, an operator or a right
  bracket, gives a complete line.

## Snapshots and results

`snapshot()` takes an immutable copy of the line's items and identities with
its source version. It does not borrow the buffer, which goes on changing,
and it keeps what it held. A computation receives a snapshot, never the
buffer, so a value, an error about a fragment of the snapshot or a
cancellation leaves the buffer as it was when the line was submitted.

## Source version

`version()` is the buffer's `SourceVersion`, from the module
`kosmo_math_model::version`. It starts at 0. Every accepted edit that changes
the items, including clearing and fragment insertion, increases it by exactly
1. Cursor and selection commands, rejected edits, edits that leave the items
as they were (such as deleting at the start of the line or replacing a
selection with the same characters), snapshots and results do not change it.
An edit that would increase it past `u64::MAX` is rejected. A snapshot keeps
the version it was taken at. The same edits on a new buffer always give the
same items, identities and versions.

## Limits

| Limit | Value | An edit past it |
| --- | --- | --- |
| `MAX_ITEMS`, the items of a line | 4,096 | is rejected with `TooManyItems`; brackets added around a fragment count |
| `MAX_INSERT_BYTES`, the bytes of one inserted text or fragment | 4,096 | is rejected with `InsertTooLong`, before its characters are checked |
| `MAX_PARSE_DEPTH`, the parse depth of a line | 64 | is rejected with `TooDeep`, also when it is a deletion |

- The parse depth is the deepest nesting of brackets and unary minus signs:
  each bracket pair and each unary minus adds a level for what it encloses,
  so `((1))` and `--1` have the depth 2. Exponents and chains of binary
  operators add none. A deletion can deepen a line: deleting the `1` of
  `1-` followed by 64 minus signs would make a chain of 65 unary minus signs.
- In one-line input blocks do not nest, so the block nesting depth is always
  1; its limit comes with two-dimensional templates.
- A line within these limits is within kalyx's default limits on input bytes
  and syntax nodes. kalyx also counts exponents and operator chains in its
  depth, so a tall tower of exponents, or brackets nested deeply inside sums
  and products, can still be rejected by kalyx as a resource limit and is
  shown as an error.
- There is no undo yet.

## Requirements

The tests are in `framework/kosmo-math-model`: the unit tests in the
`tests` modules under `src/` (named here with their module, such as
`parse::tests::…`) and the integration tests in `tests/` (named with their
file).

1. A line holds only characters of the character set; any other character is
   rejected, alone or in a text. Tests:
   `buffer::tests::the_character_set_has_72_characters`;
   `tests/limits.rs`: `a_comma_is_rejected`; `tests/operations.rs`:
   `insert_char_rejects_a_comma`,
   `insert_text_rejects_text_with_a_comma_as_a_whole`.
2. The root takes the first identity and keeps it; clearing keeps the root
   and the identity counter. Tests:
   `structure::tests::the_root_takes_the_first_identity`,
   `buffer::tests::clearing_keeps_the_root_and_the_identity_counter`;
   `tests/operations.rs`: `clear_empties_the_line`.
3. Each editing operation does what the table above states, also at the
   start and the end of the line and with a selection. Tests in
   `tests/operations.rs`, three or more for each operation:
   - `insert_char`: `insert_char_adds_a_digit_after_plus`,
     `insert_char_rejects_a_comma`,
     `insert_char_at_line_start_adds_a_unary_minus`,
     `insert_char_replaces_the_selection`;
   - `insert_text`: `insert_text_into_an_empty_line`,
     `insert_text_rejects_text_with_a_comma_as_a_whole`,
     `insert_text_at_line_end_completes_a_product`;
   - `delete_backward`: `delete_backward_removes_the_item_before_the_cursor`,
     `delete_backward_at_line_start_changes_nothing`,
     `delete_backward_at_line_end`, `delete_backward_removes_the_selection`;
   - `delete_forward`: `delete_forward_removes_the_item_after_the_cursor`,
     `delete_forward_at_line_end_changes_nothing`,
     `delete_forward_at_line_start`, `delete_forward_removes_the_selection`;
   - `move_left`: `move_left_steps_over_one_item`,
     `move_left_stops_at_line_start`, `move_left_from_line_end`,
     `move_left_goes_to_the_start_of_the_selection`;
   - `move_right`: `move_right_steps_over_one_item`,
     `move_right_stops_at_line_end`, `move_right_from_line_start`,
     `move_right_goes_to_the_end_of_the_selection`;
   - `move_to_start`: `move_to_start_from_the_middle`,
     `move_to_start_at_line_start_changes_nothing`,
     `move_to_start_from_line_end`;
   - `move_to_end`: `move_to_end_from_the_middle`,
     `move_to_end_at_line_end_changes_nothing`,
     `move_to_end_from_line_start`;
   - `extend_left`: `extend_left_selects_the_item_before_the_cursor`,
     `extend_left_at_line_start_selects_nothing`,
     `extend_left_from_line_end_to_line_start`,
     `extending_back_to_the_anchor_ends_the_selection`;
   - `extend_right`: `extend_right_selects_the_item_after_the_cursor`,
     `extend_right_at_line_end_selects_nothing`,
     `extend_right_from_line_start_to_line_end`;
   - `select_all`: `select_all_selects_the_whole_line`,
     `select_all_on_an_empty_line_selects_nothing`,
     `select_all_with_the_cursor_at_line_end`;
   - `clear`: `clear_empties_the_line`,
     `clear_on_an_empty_line_changes_nothing`,
     `clear_with_the_cursor_at_line_start`;
   - `insert_fragment`: `insert_fragment_brackets_a_sum_after_times`,
     `insert_fragment_past_the_item_limit_is_rejected`,
     `insert_fragment_at_line_start_brackets_a_sum`.
4. After every operation the cursor lies in the root between 0 and the number
   of items, and a selection has its anchor in the root, distinct from the
   cursor; a selection normalizes to a fragment. Tests:
   `structure::tests::a_selection_normalizes_to_a_fragment`;
   `tests/random_sequences.rs`: `random_edit_sequences_keep_the_invariants`.
5. A rejected edit changes nothing: not the items, the cursor, the selection
   or the source version. Tests: `tests/limits.rs`:
   `inserting_into_a_full_line_is_rejected`, `a_comma_is_rejected`;
   `tests/operations.rs`: `insert_fragment_outside_the_snapshot_is_rejected`;
   `tests/random_sequences.rs`: `random_edit_sequences_keep_the_invariants`.
6. Tokens follow the rules above: a number ends where a letter or `_` starts,
   has at most one point and a digit, and spaces only separate. A snapshot
   gives its tokens in reading order with their kinds and items, also for
   incomplete and invalid lines, and keeps them while the buffer changes.
   Tests:
   `parse::tests::a_number_ends_where_a_letter_or_underscore_starts`,
   `parse::tests::a_number_has_at_most_one_point_and_a_digit`,
   `parse::tests::spaces_only_separate_tokens`; `tests/tokens.rs`:
   `a_snapshot_gives_the_tokens_of_its_line`, `spaces_belong_to_no_token`,
   `incomplete_and_invalid_lines_have_tokens_too`,
   `the_tokens_of_a_snapshot_stay_with_it`.
7. Incomplete input stays representable, with the holes of the table above
   at the places it states. Tests: `tests/incomplete_inputs.rs`:
   `one_over_a_hole`, `a_hole_plus_a_hole`, `x_to_the_power_of_a_hole`,
   `a_left_bracket_before_a_hole`,
   `an_unclosed_bracket_after_deleting_the_right_one`,
   `an_empty_line_after_deleting_its_only_item`,
   `a_number_before_a_name_misses_an_operator`,
   `a_number_before_a_bracket_misses_an_operator`,
   `a_unary_minus_before_a_hole`, `one_plus_a_unary_minus_before_a_hole`,
   `a_left_bracket_typed_before_a_closed_pair`;
   `parse::tests::the_holes_of_the_documented_shapes`,
   `parse::tests::a_hole_sits_at_the_token_after_it_or_at_the_line_end`,
   `parse::tests::a_line_of_spaces_is_an_empty_line`,
   `parse::tests::juxtaposition_is_a_missing_operator`,
   `parse::tests::unary_plus_and_repeated_operators_miss_an_operand`.
8. Every line is exactly one of complete, incomplete and invalid, as the
   grammar reads it; an invalid part outweighs holes, and the first one is
   reported. Tests: `tests/bounded_lines.rs`:
   `every_short_line_is_classified_as_the_grammar_reads_it` (every line of up
   to 5 characters over an alphabet of 10, against a recursive-descent
   reading of the grammar written in the test);
   `parse::tests::calls_and_reserved_names_are_complete_operands`,
   `parse::tests::the_forms_of_the_grammar_are_complete`,
   `parse::tests::malformed_numbers_and_surplus_brackets_are_invalid`,
   `parse::tests::an_invalid_part_outweighs_holes_and_the_first_one_is_reported`.
9. Filling each hole of an incomplete line with what it misses gives a
   complete line. Test: `tests/bounded_lines.rs`:
   `filling_every_hole_completes_the_line`.
10. A fragment is inserted in brackets exactly when it is not a single
    operand and an operand is missing where it goes. Tests:
    `tests/operations.rs`: `insert_fragment_brackets_a_sum_after_times`,
    `insert_fragment_at_line_start_brackets_a_sum`,
    `insert_fragment_keeps_a_single_operand_as_it_is`,
    `insert_fragment_needs_no_brackets_where_no_operand_is_missing`,
    `insert_fragment_replaces_the_selection_and_brackets_where_it_was`,
    `insert_fragment_copies_part_of_the_line_itself`,
    `insert_fragment_brackets_a_fragment_whose_brackets_do_not_pair`;
    `parse::tests::single_operands_need_no_brackets`,
    `parse::tests::an_operand_is_needed_between_the_tokens_around_a_missing_one`.
11. A snapshot does not borrow the buffer and keeps what it held; a value, an
    error about a fragment of the snapshot or a cancellation leaves the
    buffer equal, field by field, to a copy taken before the submission.
    Tests: `tests/results.rs`: `a_value_leaves_the_buffer_as_it_was`,
    `an_error_about_a_fragment_leaves_the_buffer_as_it_was`,
    `a_cancellation_leaves_the_buffer_as_it_was`,
    `a_snapshot_keeps_its_content_while_the_buffer_changes`.
12. The source version starts at 0 and increases by exactly 1 for each
    accepted edit that changes the items; nothing else changes it; an edit
    past the maximum is rejected; the same edits give the same items,
    identities and versions. Tests: `tests/versions.rs`:
    `content_edits_increase_the_version_by_one`,
    `cursor_and_selection_commands_keep_the_version`,
    `rejected_edits_keep_the_version`,
    `edits_that_change_no_content_keep_the_version`,
    `the_same_edits_twice_give_the_same_structure_identities_and_versions`;
    `version::tests::starts_at_zero_and_steps_by_one`,
    `version::tests::has_no_version_after_the_maximum`,
    `buffer::tests::an_edit_past_the_maximum_version_is_rejected`;
    `tests/random_sequences.rs`: `random_edit_sequences_keep_the_invariants`.
13. An edit past the item limit or the byte limit of one insertion is
    rejected. Tests: `tests/limits.rs`:
    `inserting_into_a_full_line_is_rejected`; `tests/operations.rs`:
    `insert_fragment_past_the_item_limit_is_rejected`;
    `buffer::tests::text_of_the_byte_limit_is_accepted_and_longer_text_is_rejected`,
    `buffer::tests::text_that_would_overfill_the_line_is_rejected_whole`,
    `buffer::tests::replacing_a_selection_counts_the_items_that_remain`,
    `buffer::tests::a_fragment_of_the_byte_limit_is_accepted`,
    `buffer::tests::a_fragment_longer_than_the_byte_limit_is_rejected`,
    `buffer::tests::added_brackets_count_toward_the_item_limit`.
14. An edit past the parse depth limit is rejected, deletions included;
    exponents and operator chains add no depth. Tests:
    `buffer::tests::brackets_nest_up_to_the_depth_limit`,
    `buffer::tests::unary_minus_signs_nest_up_to_the_depth_limit`,
    `buffer::tests::exponent_towers_and_long_sums_are_within_the_depth_limit`,
    `buffer::tests::a_deletion_that_would_deepen_the_line_past_the_limit_is_rejected`;
    `parse::tests::the_parse_depth_counts_brackets_and_unary_minus_signs`,
    `parse::tests::exponents_and_operator_chains_add_no_depth`,
    `parse::tests::surplus_brackets_do_not_lower_the_depth_below_zero`.

The random sequences start from a fixed seed: 1,000 sequences of up to 64
operations drawn from all the operations above, with random characters,
texts, fragments and snapshots, including characters outside the set and
inputs that reach every limit. The constants `SEQUENCES`, `MAX_STEPS` in
`tests/random_sequences.rs` and `MAX_LENGTH` in `tests/bounded_lines.rs` set
the number of cases. When a check fails, the test reports the seed of the
sequence and how many random values it had drawn.
