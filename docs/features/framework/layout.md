# Layout and design tokens

The framework core `kosmo-ui` lays out its control tree with rows, columns
and scroll containers, aligns text by baseline, and finds the control at a
point of a layout after scrolling. Controls take their spacings, font sizes
and colours from one table of design tokens. The layout is a pure function
of the controls with their presentation states, the constraint of the root,
the table of tokens and the text metrics of a font set, and the layout pass
of each round lays every surface out with it, laying out again only what
changed. The same input gives the same layout tree byte for byte on every
run and every host, and the tests pin layout trees and one drawing as
goldens.

The layout uses the font set of [`kosmo-text`](text.md) for text and the
fixed-point geometry of `kosmo-types`, and it is written in kosmo without a
general layout library. Rounds, the shell and the drawing pass that reads
the layout are described in the [framework runtime](runtime.md). The design
rules that the layout follows are in the [layout
pass](../../architecture.md#layout-pass), [dispatch and hit
testing](../../architecture.md#dispatch-and-hit-testing) and [numeric
representation](../../architecture.md#numeric-representation) sections of
the architecture.

## Using it

A control gives its layout node with `Control::layout`, from its value and
its presentation state; by default it is `Node::Empty`. `layout(tree,
constraint, tokens, fonts)` lays the control tree `tree` out under the
`Constraint` of its root and returns a `LayoutTree`, one `TreeBox` for each
control in preorder, the drawing order. `LayoutTree::result` gives the
`LayoutResult` that hit testing and the drawing pass read, and
`LayoutTree::text` writes the tree as text. The layout pass of a round
gives each surface the layout tree of `layout`, laying out again only what
changed, and `Runtime::layout(surface)` gives it.

Rows and columns take their spacing and padding as spacing tokens, so a
length in pixels does not compile in their place. These are examples of the
crate documentation, which `cargo test` runs; the first fails to compile, as
it should:

```rust
use kosmo_types::Fx;
use kosmo_ui::Row;

let row = Row::new().spacing(Fx::from_units(512));
```

```rust
use kosmo_ui::{Row, RowAlign, Space};

let row = Row::new().spacing(Space::M).padding(Space::L).align(RowAlign::Baseline);
```

```rust
use kosmo_ui::{ColorToken, FontSize, Space, TOKENS};
use kosmo_render::Color;

assert_eq!(TOKENS.space(Space::M).units(), 8 * 64);
assert_eq!(TOKENS.font_size(FontSize::Body).units(), 16 * 64);
assert_eq!(TOKENS.color(ColorToken::Accent), Color::opaque(0x1F, 0x5F, 0xBF));
```

## The layout protocol

A parent passes each child a constraint, the smallest and the largest size
that the child may take; the child reports its size and, if it has one,
its baseline; and the parent places it. Every box takes its natural size
brought into its constraint. Where its natural size exceeds the largest
size along an axis, the box takes the largest size and marks the overflow
of that axis in the layout tree, and its children keep their natural sizes
and positions: nothing is clipped silently and nothing shrinks. The nodes
are:

| Node | What it lays out |
| --- | --- |
| `Node::Row(Row)` | The control's children from left to right |
| `Node::Column(Column)` | The control's children from top to bottom |
| `Node::Scroll(Scroll)` | The control's first child, its content, moved by the scroll offset along one axis |
| `Node::Text(Text)` | A paragraph in one of the two fonts at a font size token, drawn in a colour token |
| `Node::Fixed(Size)` | A leaf of this size |
| `Node::Empty` | A leaf of size zero, the node of a control that gives none |

Text and fixed-size leaves draw; rows, columns, scroll containers and empty
leaves do not, so the drawing pass does not call their `Control::draw` and
hit testing never finds them. A leaf's children, and the children of a
scroll container after its first, are laid out with the tight constraint of
size zero and placed at the origin of their parent, so they take no room.

All coordinates are `Fx` values in 1/64 pixel. A constraint, the size of a
fixed-size leaf or of a paragraph and the absolute value of a scroll
request are at most `MAX_LAYOUT_INPUT`, $2^{18}$ pixels, and so are the
spacings of the table of tokens, whose font sizes must be sizes that
`kosmo-text` lays out. An input beyond its range, a constraint whose
smallest size exceeds its largest, or a table of tokens out of its range
makes the whole layout return `LayoutError::InputOutOfRange`, and a sum of
lengths, or a position that the text layout gives, that does not fit makes
it return `LayoutError::Overflow`; the layout never panics.

## Rows and columns

`Row::new()` places its children side by side, each at its own size, from
its left padding, with its spacing between them; `Column::new()` does the
same from top to bottom. `spacing(space)` and `padding(space)` take a
`Space` token, and without them a row or column has neither. A row passes
each child a width from 0 to `MAX_LAYOUT_INPUT` and a height from 0 to its
own largest height less its padding above and below, and a column does the
same across the other axis. The natural size of a row or column is the
size of its children with the spacing and padding.

Children are aligned across a row by `RowAlign::Start`, the default,
`Center` or `End`, and across a column by `Align::Start`, `Center` or
`End`. A centred child takes half its free room, rounded by
`kosmo_types::round`. Nothing stretches, and children do not wrap.

## Baselines

A text leaf has the baseline of its first line. A row or column has the
baseline of its first child that has one, moved by the child's position;
fixed-size and empty leaves have none. A row aligned by
`RowAlign::Baseline` puts the baselines of its children at one height, and
a child without a baseline puts its bottom edge there; its content is then
as tall as the largest height above the baseline and the largest height
below it together. Text of two sizes or two fonts, a column of lines and a
paragraph of several lines thus share the baseline of their first lines.

## Text leaves

`Text::new(text, font, size)` lays out `text` in `Font::Sans`, Fira Sans,
or `Font::Mono`, Fira Mono, at the `FontSize` token `size`, and
`color(color)` names its `ColorToken`, by default `ColorToken::Text`. The
paragraph breaks its lines at the largest width of the leaf's constraint
and at the line breaks of its text, and its natural size is the size of the
paragraph. The layout tree keeps the paragraph, as a `LaidText`, for the
drawing pass, and `DrawContext::text` hands it to the control.

## Scroll containers

`Scroll::new(axis, request)` lays out its first child, the content, with a
length from 0 to `MAX_LAYOUT_INPUT` along `axis`, `Axis::X` or `Axis::Y`,
and up to its own largest size across it, and takes the content's size
within its own constraint, without marking overflow; content wider than a
vertical container marks its own overflow. Its offset is the request
brought into the range from 0 to the content's length less its own, or 0
when the content is not longer. Its children are moved by the offset and
clipped to its box when hit testing and the drawing pass place them.

The request is the control's to keep, usually in its presentation state.
After the layout pass of a round, `Control::scrolled(offset, state)` tells
each container's control the offset it got; a control that keeps its
request in its presentation state writes the offset back there, so that
requests beyond the range do not add up, and the layouts that the runtime
keeps follow the requests written back. The default writes nothing. A
change of the request alone moves only the container's content and lays
nothing out.

## Hit testing after scrolling

`hit_test(layout, point)` returns the path from the root to the control at
`point` of a surface, or `None`: each box is moved by the scroll offsets of
its ancestors and clipped by their boxes, holds its left and top edges but
not its right and bottom edges, and of overlapping boxes the last in
drawing order is hit; a box that does not draw is never hit. After a
vertical container of 100 × 50 pixels over six rows of 20 pixels has
scrolled by 30 pixels, a point 5 pixels into the viewport hits the second
row and a point 45 pixels into it the fourth; a request of 500 pixels
scrolls to the end of the range, 70 pixels, and a point on the bottom edge
of the viewport hits nothing. [Hit testing](runtime.md#hit-testing)
describes the function.

## Design tokens

`Tokens` is a table of the value of each token, and `TOKENS` is kosmo's one
table:

| Token | Value |
| --- | --- |
| `Space::Xs` | 2 pixels |
| `Space::S` | 4 pixels |
| `Space::M` | 8 pixels |
| `Space::L` | 16 pixels |
| `FontSize::Small` | 12 pixels per em |
| `FontSize::Body` | 16 pixels per em |
| `FontSize::Large` | 24 pixels per em |
| `ColorToken::Background` | `#FFFFFF` |
| `ColorToken::Text` | `#1A1A1A` |
| `ColorToken::TextSecondary` | `#595959` |
| `ColorToken::Accent` | `#1F5FBF` |
| `ColorToken::Error` | `#B3261E` |

Spacings and font sizes are `Fx` values, and colours opaque 8-bit sRGB
constants. Controls take values only through the tokens: rows and columns
take `Space` tokens and text leaves a `FontSize` and a `ColorToken`, so a
length in pixels does not compile in their place, and a control draws with
the colour that `DrawContext::color(token)` gives. The drawing pass checks
every command that a control draws against the table of its round: a
colour that is not the value of a colour token, or a glyph run at a size
that is not the value of a font size token, makes it return
`PaintError::Untokened` with the control, and the surface gets no frame,
as [the drawing pass](runtime.md#the-drawing-pass) describes.

The shell lays out and draws every round with `TOKENS`. A round takes its
table from its `LayoutInput`, so a test can lay out and draw with a changed
copy, such as `Tokens { space_m: Fx::from_units(640), ..TOKENS }`; another
table lays every surface out whole.

## Layout tree goldens

A layout tree is written as UTF-8 text by `LayoutTree::text`: the line
`layout-tree 1`, the format and its version, and then one line for each box
in preorder, with its index, its parent's index, the kind and key of its
control, the smallest and largest size of its constraint, its position, its
size, its baseline, its scroll offset, the axes along which it overflows and
whether it draws. Every number is an integer, a count, a key or a value in
1/64 pixel, so a layout tree holds no NaN and no infinity.

The goldens of `kosmo-ui` are in `framework/ui/tests/golden/`, listed in
`MANIFEST` with their SHA-256 digests:

| Golden | What it holds |
| --- | --- |
| `layout-row-spacing`, `layout-row-cross`, `layout-row-padding`, `layout-row-overflow`, `layout-row-tight` | Rows with spacing, aligned at the start, centre and end, with padding, wider than their largest width and under a tight constraint |
| `layout-column-spacing`, `layout-column-cross`, `layout-column-wrap`, `layout-column-overflow` | Columns with spacing, aligned at the start, centre and end, with text broken at their width and taller than their largest height |
| `layout-scroll-y`, `layout-scroll-y-offset`, `layout-scroll-x`, `layout-scroll-cross` | Scroll containers at offset 0, at an offset in range, along `x`, and with content wider than a vertical viewport |
| `layout-baseline-sizes`, `layout-baseline-fonts`, `layout-baseline-box`, `layout-baseline-nested`, `layout-baseline-lines` | Text of two sizes, of two fonts, beside a box without a baseline, in a nested column and of several lines on one baseline |
| `layout-tokens` | The layout tree of the scene of the design tokens |
| `tokens` | The pixels of the scene of the design tokens |

The first 18 are layout trees of the layout cases, each a control tree laid
out by `layout` under the constraint of its root with `TOKENS` and the font
set of the two bundled fonts; the cases are in
`framework/ui/src/layout/tests/cases.rs`, which the unit tests share. The
scene of the design tokens uses all twelve: a column with the padding
`Space::L` and the spacing `Space::M` of the title "Result" at
`FontSize::Large`; a row aligned by baseline with the spacing `Space::Xs`
of "x = 1/3" in Fira Mono at `FontSize::Body` and "stale" at
`FontSize::Small` in `ColorToken::TextSecondary`; a rule of 208 × 2 pixels
in `ColorToken::Accent`; and a row with the spacing `Space::S` of "error:"
and "division by zero" at `FontSize::Body` in `ColorToken::Error`; the
other text is in `ColorToken::Text` and in Fira Sans. The first round of a
new runtime lays it out on a surface of 240 × 160 pixels and draws it, and
vello_cpu draws a rectangle of the surface in `ColorToken::Background` and
over it the round's frame from the coverage masks of the glyph cache, into
a tightly packed `Rgba8Premul` canvas encoded as the pixel goldens of
[rendering](rendering.md#pixel-goldens) are. Each golden test lays out or
draws its golden alone and compares the result with the file byte for
byte, without tolerance.

Goldens change only through `cargo xtask golden`. After a change of the
layout, of the drawing or of the table of tokens, the first command below
writes the goldens of every crate with goldens and their manifests again,
and leaves every file as it was when nothing changed; add a new golden's
file with `git add`. The second lays out and draws them again into
`framework/target/golden/kosmo-ui/` and prints each digest with the digest
in `MANIFEST`, changing no tracked file. A golden edited by hand fails the
check: the goldens step of `cargo xtask check` reports a file whose digest
differs from `MANIFEST`, and the golden's test a file that differs from its
layout or drawing. Review reads the change of a layout tree golden line by
line beside the change of code that caused it.
[The goldens step and `cargo xtask
golden`](../../../CONTRIBUTING.md#the-goldens-step-and-cargo-xtask-golden)
states the rules.

```sh
cargo xtask golden
cargo xtask golden --verify
```

## The layout pass in a round

`Round::finish_with(app, input)` runs the layout pass after the view pass,
for each surface of its `LayoutInput`, in the order of the surfaces, with
the surface's size as the tight constraint of the root, the input's table
of tokens and its font set; the shell gives its own font set, `TOKENS` and
the size of each surface's latest `Configure`. The runtime keeps each
surface's layout, and the next layout pass lays out only dirty subtrees
against it:

- a control is dirty when it is new, when the layout node that it gives
  differs from the round before other than in a scroll request, or when its
  children differ;
- a subtree without a dirty control whose root gets the constraint of the
  round before keeps its layout whole;
- a box whose control is not dirty keeps its size and its children's
  positions while its children come out with the sizes and baselines of
  the round before, so the work goes up from a dirty control only while
  sizes or baselines change, and stops at the latest at a box under a
  tight constraint whose baseline stays, a layout boundary;
- another size of the surface, another table of tokens or another font set
  lays the surface out whole.

The result is that of `layout` byte for byte. Then each scroll container's
control learns its offset through `Control::scrolled`, and the drawing pass
draws each surface whose layout tree or controls changed since its latest
frame. [Rounds](runtime.md#rounds) describes the pass, its records and its
errors.

## Limits

- The first release has rows, columns and scroll containers only: no
  grids, no flexible sizes, no stacking, and no stretching or wrapping of
  children. Rows, columns and scroll containers do not draw, so a
  background behind them is drawn only by a leaf or outside the drawing
  pass.
- A leaf's children, and the children of a scroll container after its
  first, are laid out under the tight constraint of size zero and placed at
  the origin of their parent: they take no room, and the layout reports no
  error for them.
- A scroll container scrolls along one axis, and only its control moves
  its request: the minimal key map has no scrolling commands, so a control
  that scrolls by keys binds its own commands in its binding table. The
  offset is written back only by a control that implements
  `Control::scrolled`.
- There is one table of design tokens, `TOKENS`, and no theme: nothing
  switches tables while an application runs, and there are no tokens of
  radii, strokes, line heights or other properties.
- The drawing pass checks the values of colours and glyph sizes, not where
  they come from: a literal equal to the value of a token passes, as one
  taken with `DrawContext::color` does, and only a value outside the table
  is refused.
- The groups of the drawing pass count toward the limit of the display
  list, `MAX_COMMANDS` of `kosmo-render`, 16,384 commands. In a layout tree
  only text and fixed-size leaves draw, so a frame in which `n` leaves draw
  `c` commands in all and `m` boxes have a child visible in the damage
  holds `c + 2n + 2m` commands; beyond the limit the round gives the
  surface no frame and records `too-many-commands`, as
  [rounds](runtime.md#rounds) state.
- The update region of a surface is the whole surface or nothing: rounds
  do not work out which part of a surface changed, so a change of one
  control draws the whole surface again.
- The layout pass takes the layout node of every control in every round to
  find the dirty ones; only the laying out is limited to dirty subtrees.
- Text is laid out from left to right in the two bundled fonts, without
  fallback, as [text](text.md) states.
- Hit testing has no caller outside tests: there are no pointer events.

## Requirements

The tests are in `framework/ui`: the unit tests in the `tests` modules under
`src/` (named here with their module, such as `layout::tests::…`) and the
integration tests in `tests/` (named with their file); the goldens step
is tested in `xtask/`. No test sleeps or reads the wall clock.

1. A row places its children with its spacing between them and its
   padding around them, aligns them at the start, centre or end, centres a
   child with half its free room rounded up, takes its largest width and
   marks overflow along `x` when it is wider, and takes the size of a tight
   constraint; their layout trees are the goldens. Tests:
   `layout::tests::` `a_row_puts_its_spacing_between_its_children`,
   `rows_align_their_children_at_the_start_centre_or_end`,
   `a_centred_child_takes_half_its_free_room_rounded_up`,
   `a_row_keeps_its_padding_around_its_children`,
   `a_row_wider_than_its_largest_width_takes_it_and_marks_overflow_in_x`,
   `a_row_under_a_tight_constraint_takes_its_size`; `tests/golden.rs`:
   `a_row_with_spacing_matches_its_golden`,
   `rows_aligned_at_the_start_centre_and_end_match_their_golden`,
   `a_row_with_padding_matches_its_golden`,
   `a_row_wider_than_its_largest_width_matches_its_golden`,
   `a_row_under_a_tight_constraint_matches_its_golden`.
2. A column places its children with its spacing, aligns them across it,
   breaks the text of a leaf at its width, and takes its largest height
   and marks overflow along `y` when it is taller; their layout trees are
   the goldens. Tests: `layout::tests::`
   `a_column_puts_its_spacing_between_its_children`,
   `columns_align_their_children_at_the_start_centre_or_end`,
   `children_align_in_a_box_larger_than_they_are`,
   `text_in_a_column_breaks_at_the_columns_width`,
   `a_column_taller_than_its_largest_height_takes_it_and_marks_overflow_in_y`;
   `tests/golden.rs`: `a_column_with_spacing_matches_its_golden`,
   `columns_aligned_at_the_start_centre_and_end_match_their_golden`,
   `text_broken_at_the_width_of_its_column_matches_its_golden`,
   `a_column_taller_than_its_largest_height_matches_its_golden`.
3. A scroll container starts at offset 0, brings its request into the
   range of its content, moves only its content when the request is in
   range, lets its content grow past the viewport along its axis, and
   leaves the overflow across it to its content; their layout trees are
   the goldens. Tests: `layout::tests::`
   `a_vertical_scroll_container_starts_at_offset_zero`,
   `an_offset_within_its_range_changes_only_the_scroll_offset`,
   `a_horizontal_scroll_container_lets_its_row_grow_past_the_viewport`,
   `content_wider_than_a_vertical_viewport_marks_overflow_in_x`,
   `a_scroll_request_is_brought_into_the_range_of_its_content`;
   `tests/golden.rs`:
   `a_vertical_scroll_container_at_offset_zero_matches_its_golden`,
   `a_vertical_scroll_container_at_an_offset_in_range_matches_its_golden`,
   `a_horizontal_scroll_container_matches_its_golden`,
   `content_wider_than_a_vertical_viewport_matches_its_golden`.
4. A row aligned by baseline puts text of two sizes and of two fonts on
   one baseline, puts the bottom of a box without a baseline there, takes
   the baseline of a nested column from its first child and the first
   baseline of a paragraph of several lines; their layout trees are the
   goldens. Tests: `layout::tests::`
   `text_of_two_sizes_shares_one_baseline`,
   `text_of_two_fonts_shares_one_baseline`,
   `a_box_without_a_baseline_puts_its_bottom_on_the_baseline`,
   `a_column_reports_the_baseline_of_its_first_child`,
   `text_of_several_lines_aligns_its_first_baseline`; `tests/golden.rs`:
   `text_of_two_sizes_on_one_baseline_matches_its_golden`,
   `text_of_two_fonts_on_one_baseline_matches_its_golden`,
   `a_box_without_a_baseline_beside_text_matches_its_golden`,
   `the_baseline_of_a_nested_column_matches_its_golden`,
   `text_of_several_lines_on_one_baseline_matches_its_golden`.
5. A control without a layout node is an empty leaf, the children that a
   node does not lay out take no room, and a text leaf without a colour
   takes the colour `Text`. Tests: `layout::tests::`
   `a_control_without_a_layout_is_an_empty_leaf`,
   `children_that_their_node_does_not_lay_out_take_no_room`,
   `a_text_leaf_without_a_colour_takes_the_text_token`.
6. The layout is a pure function: each case laid out twice, and through a
   second font set of the same bytes, gives the bytes of its golden, and in
   1,000 random trees from fixed seeds it never panics, every box keeps
   within its constraint and every child within its parent, and the two
   layouts of each tree write the same text. Tests:
   `layout::tests::every_case_laid_out_twice_writes_the_same_bytes`;
   `tests/golden.rs`:
   `every_case_laid_out_twice_writes_the_bytes_of_its_golden`,
   `a_second_font_set_of_the_same_bytes_writes_the_same_goldens`;
   `layout::tests::random::random_trees_lay_out_within_their_constraints_without_panicking`.
7. An input beyond its range, a paragraph wider than the largest input
   and a length beyond the numeric representation fail the whole layout
   with their distinct errors. Tests: `layout::tests::`
   `inputs_beyond_their_range_are_refused_distinctly`,
   `a_paragraph_wider_than_the_largest_input_is_refused`,
   `a_length_beyond_the_numeric_representation_fails_the_whole_layout`.
8. The text of a layout tree has a line for each box in preorder, its
   layout result holds the boxes, every golden has its file, and a golden
   one line off its layout, or with other line ends, fails with the line
   that differs; the goldens step reports a layout tree golden edited by
   hand, one that `MANIFEST` lacks and one that no test refers to. Tests:
   `layout::tests::the_text_of_a_tree_has_a_line_for_each_box_in_preorder`,
   `layout::tests::the_layout_result_holds_the_boxes_of_the_tree`;
   `tests/golden.rs`: `every_case_has_a_golden_of_its_name`,
   `a_golden_file_one_line_off_its_layout_fails`,
   `a_golden_file_that_differs_only_in_its_line_ends_fails`,
   `the_helper_of_the_comparisons_fails_without_a_case`;
   `xtask/src/golden/tests.rs`:
   `layout_tree_goldens_with_their_files_lines_and_tests_pass`,
   `a_hand_edited_layout_tree_golden_is_reported`,
   `a_layout_tree_file_that_the_manifest_lacks_is_reported`,
   `a_layout_tree_golden_that_no_test_references_is_reported`.
9. A layout against the layout before lays out only what changed and
   equals the whole layout, also after random changes, and another
   constraint, table of tokens or font set lays the whole tree out; the
   layout pass of a round does so for each surface and writes the offsets
   back. Tests: `layout::tests::`
   `a_layout_against_the_layout_before_lays_out_only_what_changed`,
   `a_request_beyond_its_range_in_a_subtree_taken_whole_fails_as_the_whole_layout`,
   `another_constraint_table_of_tokens_or_font_set_lays_out_the_whole_tree`;
   `layout::tests::random::layouts_against_the_layout_before_equal_whole_layouts_after_random_changes`;
   and the tests of the layout pass that requirement 20 of the [framework
   runtime](runtime.md#requirements) names.
10. The table of design tokens holds the values of the table above, its
    font sizes are sizes that `kosmo-text` lays out, and a length in pixels
    does not compile as a spacing. Tests:
    `tokens::tests::the_table_holds_the_values_of_the_tokens`,
    `tokens::tests::the_font_sizes_are_sizes_that_kosmo_text_lays_out`; the
    examples of `Tokens` and `Row`, which `cargo test` runs.
11. Controls take their colours only from the design tokens: a control
    that draws a colour of no token, or a glyph run at a size of no font
    size token, makes the drawing pass return `PaintError::Untokened` and
    no display list. Tests: `paint::tests::`
    `a_colour_outside_the_table_of_tokens_is_refused`,
    `a_glyph_run_at_a_size_outside_the_font_size_tokens_is_refused`,
    `the_check_compares_values_not_where_they_come_from`.
12. The scene of the design tokens uses every token: its layout tree and
    its pixels, laid out and drawn by a round through the drawing pass
    with the check of the tokens, vello_cpu and the glyph cache, match
    their goldens byte for byte, and a change of any one of the twelve
    tokens changes its layout tree or its pixels. Tests: `tests/golden.rs`:
    `the_layout_tree_of_the_scene_of_the_design_tokens_matches_its_golden`,
    `the_scene_of_the_design_tokens_matches_its_pixel_golden`,
    `each_token_of_the_table_changes_the_scene_of_the_design_tokens`.
13. A changed table of tokens changes the goldens only through
    `cargo xtask golden`: the scene laid out and drawn with the spacing
    `M` of 10 pixels and the accent colour `#C00000` has another layout
    tree and other pixels than its goldens, with its rule 4 pixels lower
    and red, and with `TOKENS` it has those of the goldens. Test:
    `tests/golden.rs`:
    `a_changed_table_of_tokens_changes_the_layout_tree_and_the_drawing_of_the_scene`.
14. After scrolling, hit testing on the layout result finds the control
    under a point: in a vertical container of 100 × 50 pixels over six
    rows of 20 pixels, a request of 30 pixels gives the offset 30, where
    (10, 5) hits the second row and (10, 45) the fourth; a request of 500
    gives the end of the range, 70, where (10, 45) hits the sixth row and
    (10, 0) the fourth; a request of -30 gives 0; content that fits its
    viewport does not scroll; and the bottom edge of the viewport hits
    nothing. Tests: `tests/scroll.rs`:
    `after_a_request_within_the_range_points_of_the_viewport_hit_the_rows_under_them`,
    `a_request_beyond_the_range_scrolls_to_its_end_where_points_hit_the_last_rows`,
    `a_negative_request_scrolls_to_the_start`,
    `content_that_fits_its_viewport_does_not_scroll`,
    `a_point_below_the_viewport_hits_no_row`.
15. The same two requests through rounds: a container moves its request by
    a command that its binding table binds to a key, the round lays it out
    with the request and writes the offset back, hit testing on the
    round's layout finds the same rows as on the layout of the layout
    function, and a further request beyond the range leaves the request at
    its end. Test: `tests/scroll.rs`:
    `bound_commands_scroll_in_rounds_and_hit_testing_reads_the_layout_of_the_round`.

## Running the tests

From the repository root, the first command runs every test of
`kosmo-ui`, the next two the goldens and the scrolling tests alone, and
the last lays out and draws the goldens again and compares their digests
with `MANIFEST`:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-ui
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-ui --test golden
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-ui --test scroll
cargo xtask golden --verify
```
