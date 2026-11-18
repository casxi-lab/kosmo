# Formula conversion

The library `kosmo-math-kalyx` in the `framework/` workspace turns a line of
[one-line formula input](one-line-editing.md) into the text that kalyx reads,
maps the ranges of kalyx's errors back to the line, and reads the text of a
value back into a line of its own, from which a copy can be reused. It
follows kalyx 0.1.0 from crates.io, the version that `framework/Cargo.lock`
locks, and it is the only crate of kosmo that depends on kalyx: the layout
step of `cargo xtask check` fails when any other crate, or `xtask`, declares
it.

Submitting the text as a job and running it with kalyx are planned for the
job layer `kosmo-jobs` and the kalyx runner of this crate, and reading the
output lines of kalyx into results for the display model of
`kosmo-math-model` and this crate. The design rules are in
[formula conversion](../../architecture.md#formula-conversion) and
[holes and completeness](../../architecture.md#holes-and-completeness) in
the architecture.

## Using it

The library has three functions. `convert` takes a snapshot of a line and
gives its linear text, or the reason why there is none. `map_span` takes the
same snapshot and the span of a kalyx error and gives the place in the line
that the error is about. `read_back` takes the display text of a value and
gives a snapshot of a separate line, or the reason why the value cannot be
reused. The crate documentation has an example, which `cargo test` runs.
Run the library's tests from the repository root with:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-math-kalyx
```

## Linear text

Only a complete line is converted. An incomplete line gives its holes and an
invalid line its first invalid part, as
[holes and completeness](one-line-editing.md#holes-and-completeness)
describes, and neither gives any text, so nothing is computed from them.

The linear text of a complete line is its characters as you typed them,
spaces included, with two changes to numbers, because kalyx 0.1.0 reads only
numbers of the forms `1` and `1.5`: a number that starts with a point gets a
`0` before it, and a number that ends with a point loses the point.

| Line | Text |
| --- | --- |
| `1.5 +  12` | `1.5 +  12` |
| `.5+1.` | `0.5+1` |
| `x^.5` | `x^0.5` |
| `2*(x+1)`, after inserting the line `x+1` after `2*` | `2*(x+1)` |
| `pi(x)` | `pi(x)` |

- Names are split as text: letters, digits and `_` that follow a letter or
  `_` belong to one name, so `xy` is one name, not $x \cdot y$. Names are
  case-sensitive, and `Pi`, `ii`, `pi2`, `e` and command words such as `diff`
  are ordinary names.
- Calls, a name followed by a bracket with spaces allowed between, and the
  reserved names `pi`, `i` and `undefined` are written as typed. kalyx 0.1.0
  reports them as unsupported: a call with the range from its name to its
  left bracket, and a reserved name with the range of the name, also when a
  bracket follows it, as in `pi(x)`.
- A complete line holds no juxtaposition such as `2x`, no unary plus and only
  ASCII characters, so its text uses only the grammar of kalyx 0.1.0, and
  kalyx reads it with the precedence and associativity that the editing
  structure reads: `1+2*3` is 7, `2^3^2` is 512, `-2^2` is $-4$, `2^-1^2` is
  $1/2$ and `-2^2 + 2^3^2` is 508.

## Error ranges

An error line of kalyx 0.1.0 gives the span of the error: a half-open range
of bytes of the text, `start..end`, which may be empty, or `-` for an error
that has no range, such as an unknown symbol in an evaluation. The span maps
back to the line:

- The text is made of pieces, one for each token and each space of the line.
  A non-empty span maps to the items from the first item of the piece that
  holds its first byte to the end of the piece that holds its last byte. A
  span over a number therefore covers all of the number's items, the point
  that conversion left out or wrote a `0` before included. kalyx 0.1.0
  reports spans that start and end at the edges of tokens; only a non-empty
  span that starts or ends inside a piece grows to the edges of that piece.
- An empty span always maps to a position between two items: at the edge of
  a piece, the position there, and inside a piece, the position before the
  piece. `-` maps to the whole line. A span that runs backwards or past the
  end of the text is rejected.

In this table, the items are a half-open range of the line, like the span:

| Line | Text | Error | Span | Items |
| --- | --- | --- | --- | --- |
| `0*(1/0)` | `0*(1/0)` | division by zero | `3..6` | `3..6`, `1/0` |
| `a*b/(x-x)*c` | `a*b/(x-x)*c` | division by zero | `0..9` | `0..9`, `a*b/(x-x)` |
| `.5+1./0` | `0.5+1/0` | division by zero | `4..7` | `3..7`, `1./0` |
| `2/0.` | `2/0` | division by zero | `0..3` | `0..4`, `2/0.` |
| `x^.5` | `x^0.5` | exponent not an integer | `2..5` | `2..4`, `.5` |
| `f (x)` | `f (x)` | unsupported call | `0..3` | `0..3`, `f (` |
| `x` | `x` | unknown symbol in an evaluation | `-` | the whole line |

The same mapping works on a line that is not complete, with the span taken
over the line written by the same rules. Such a line is never converted, but
if its characters are given to kalyx as they are, its syntax errors map to
its holes and its invalid part: the empty span at the end of `1+` to the
position of the missing operand; the error of `2(x+1)` to its `(`, which
starts at the position of the missing operator; and the error of `1)` to the
surplus `)`.

## Reading back

The display text of a value is read back by typing it into a new line as one
insertion. When that line is complete, its snapshot is returned, and a copy
of it can be inserted into the line you are editing, with brackets where
fragment insertion adds them. Its linear text is the display text itself, and
kalyx 0.1.0 reads it as the same term.

A value whose text the editing structure rejects, or whose line is not
complete, cannot be reused, and the reason says why: for example a text
longer than the 4,096 bytes of one insertion. Nothing is inserted anywhere.

## Limits

- The rules hold for kalyx 0.1.0. When kosmo moves to a later kalyx version,
  the writing of numbers, calls and reserved names and the meaning of spans
  are checked again against that version's grammar and output lines.
- Reading back is bounded by the limits of the editing structure: one
  insertion holds at most 4,096 bytes and a line at most 4,096 items. Of the
  49 answers of kalyx 0.1.0's answer vectors that are values, two, of 4,749
  and 10,567 bytes, are too long to be reused; the other 47 read back.
- Conversion adds one byte for each number that starts with a point, so the
  text of a full line of 4,096 items has at most 5,461 bytes, far within
  kalyx's default limit of 65,536 input bytes. kalyx counts exponents and
  operator chains in its depth limit, which the editing structure does not,
  so a complete line such as `2` followed by 128 times `^2` is reported by
  kalyx as a resource limit, at the `^` that opens the level past the limit.
- kalyx reads display text under the limits of each request like any other
  input, so the text of a value near those limits reads back as a line but is
  read by kalyx only under larger limits.
- One-line input holds only characters. Two-dimensional templates are
  converted when two-dimensional editing adds them.
- The tests read a copy of kalyx 0.1.0's answer vectors in
  `framework/kosmo-math-kalyx/tests/data/`, whose README states its source and
  checksum; they do not check that the copy matches the locked version.

## Requirements

The tests are in `framework/kosmo-math-kalyx`: the unit tests in the `tests`
modules under `src/` (named here with their module, such as
`conversion::tests::…`) and the integration tests in `tests/` (named with
their file), which read text with kalyx 0.1.0. The tests of the layout step
are in `xtask/src/layout/tests.rs`.

1. Only a complete line is converted: an incomplete line gives exactly its
   holes, an invalid line its first invalid part, and neither gives text.
   Tests: `tests/incomplete_inputs.rs`: `one_over_a_hole`,
   `a_hole_plus_a_hole`, `x_to_the_power_of_a_hole`,
   `a_left_bracket_before_a_hole`,
   `an_unclosed_bracket_after_deleting_the_right_one`,
   `an_empty_line_after_deleting_its_only_item`,
   `a_number_before_a_name_misses_an_operator`,
   `a_number_before_a_bracket_misses_an_operator`,
   `a_unary_minus_before_a_hole`, `one_plus_a_unary_minus_before_a_hole`,
   `a_left_bracket_typed_before_a_closed_pair`; `tests/random_lines.rs`:
   `random_incomplete_lines_convert_to_their_holes_only`;
   `conversion::tests::incomplete_lines_give_their_holes_and_no_text`,
   `conversion::tests::invalid_lines_give_their_invalid_part_and_no_text`.
2. The text is the line as typed, spaces included, with a `0` before a
   number that starts with a point and without the point that ends a number.
   Tests: `tests/complete_inputs.rs`:
   `numbers_are_written_as_kalyx_reads_them`;
   `conversion::tests::numbers_with_a_point_at_either_end_are_normalized`,
   `conversion::tests::everything_but_numbers_is_written_as_typed`.
3. kalyx 0.1.0 reads the text of a complete line as the reading of the
   editing structure: names split as text, the precedence and associativity
   of each operator, and the brackets that fragment insertion adds. Tests:
   `tests/complete_inputs.rs`: `names_are_split_as_text`,
   `precedence_and_associativity_follow_the_grammar`,
   `a_unary_minus_and_a_tower_of_powers_in_one_line`,
   `brackets_added_by_fragment_insertion_are_written`.
4. Calls and reserved names are written as typed, and kalyx 0.1.0 reports
   them as unsupported with the range of the call's name and left bracket or
   of the reserved name. Tests: `tests/complete_inputs.rs`:
   `reserved_names_and_calls_are_passed_on_unchanged`;
   `tests/error_spans.rs`: `reserved_names_and_calls_map_to_their_writing`.
5. No complete line gives a syntax error in kalyx 0.1.0, and only lines with
   a call or a reserved name are unsupported. Test: `tests/random_lines.rs`:
   `random_complete_lines_convert_to_text_that_kalyx_reads`.
6. The span of an error maps back by pieces, so a span over a number covers
   all its items; on a line written as typed it covers the same items as its
   bytes. Tests: `tests/error_spans.rs`:
   `a_division_by_zero_maps_to_the_chain_up_to_the_divisor`,
   `an_exponent_that_is_not_an_integer_maps_to_the_exponent`,
   `spans_over_normalized_numbers_cover_all_their_items`,
   `a_tower_past_the_depth_limit_maps_to_the_operator_that_opens_the_level`;
   `tests/random_lines.rs`:
   `random_complete_lines_convert_to_text_that_kalyx_reads`;
   `conversion::tests::each_token_and_each_space_is_one_piece`,
   `conversion::tests::the_pieces_cover_every_item_and_byte_in_order`,
   `span::tests::a_span_of_a_line_written_as_typed_covers_the_same_items`,
   `span::tests::a_span_over_a_normalized_number_covers_all_its_items`,
   `span::tests::a_span_that_starts_or_ends_inside_a_piece_grows_to_its_edges`,
   `span::tests::spaces_are_pieces_of_their_own`.
7. An error without a span maps to the whole line, an empty span always to
   a position, also inside a piece, and a span outside the text is rejected.
   Tests: `tests/error_spans.rs`:
   `an_error_without_a_span_maps_to_the_whole_line`;
   `span::tests::an_error_without_a_span_is_about_the_whole_line`,
   `span::tests::an_empty_span_marks_the_position_between_pieces`,
   `span::tests::an_empty_span_inside_a_piece_marks_the_position_before_it`,
   `span::tests::a_span_outside_the_text_is_rejected`.
8. The syntax errors of a line that is not complete, given to kalyx as it
   is, map to its holes and its invalid part. Test: `tests/error_spans.rs`:
   `syntax_errors_of_the_line_itself_map_to_its_holes_and_invalid_part`.
9. The answers of kalyx 0.1.0's answer vectors that are values and fit one
   insertion, 47 of them, read back into complete lines whose text kalyx
   reads as the same terms, and so does the display text of each of those
   values. Tests: `tests/read_back.rs`:
   `the_answer_vectors_hold_forty_nine_ok_answers`,
   `answers_within_one_insertion_read_back_and_convert_to_the_same_terms`,
   `the_display_text_of_each_value_reads_back`;
   `read_back::tests::display_text_reads_back_into_a_complete_line`.
10. A value that does not read back into a complete line is not reusable,
    with the reason, and nothing is inserted. Tests: `tests/read_back.rs`:
    `answers_longer_than_one_insertion_are_not_reusable`;
    `read_back::tests::text_longer_than_one_insertion_is_not_reusable`,
    `read_back::tests::text_that_the_editing_structure_rejects_is_not_reusable`,
    `read_back::tests::text_that_is_not_a_complete_line_is_not_reusable`.
11. No crate other than `kosmo-math-kalyx`, and not `xtask`, declares kalyx.
    Tests: `xtask/src/layout/tests.rs`:
    `rejects_kalyx_in_the_dev_dependencies_of_the_math_model`,
    `no_other_crate_declares_kalyx_in_any_table`,
    `crates_of_other_workspaces_may_not_declare_kalyx`,
    `kalyx_is_found_by_its_package_name_when_renamed`,
    `an_entry_that_is_not_inherited_is_not_named_by_the_root`,
    `a_package_with_a_name_like_kalyx_is_not_kalyx`,
    `a_key_named_kalyx_that_renames_another_package_is_not_kalyx`,
    `an_inherited_kalyx_is_kalyx_whatever_package_the_entry_names`,
    `an_inherited_entry_naming_kalyx_is_not_kalyx_when_its_root_declares_another`,
    `xtask_may_not_declare_kalyx`,
    `the_kalyx_adapter_may_declare_kalyx_in_any_table`,
    `crates_may_reach_kalyx_through_the_adapter`.

The random lines start from a fixed seed: 1,000 complete lines written from
the grammar, with numbers that start or end with a point, spaces, calls and
reserved names, and 1,000 incomplete lines made from such lines by deleting,
inserting or cutting off characters. The constant `LINES` in
`tests/random_lines.rs` sets the number of cases. When a check fails, the
test reports the seed of the line and how many random values it had drawn.
