# Text

The library `kosmo-text` in the `framework/` workspace holds the two fonts
that kosmo bundles, Fira Sans and Fira Mono, with their license files and
their asset manifest; the font set `FontSet`, which registers the bytes of
both fonts for the text stack parley, without system fonts; the layout of
paragraphs into lines and glyph runs of display lists; and the glyph cache
`GlyphCache`, which holds the coverage masks that the rasterizer of
`kosmo-render` draws glyphs from. The same text in the same style gives the
same glyphs, the same lines and the same pixels on every run and every host,
and the tests pin them as text and as pixel goldens.

The shell of `kosmo-ui` fetches the two fonts through the platform interface
before its first round, builds its font set from their bytes, and prepares
the glyph runs of each frame in the glyph cache before the rasterizer draws
it, as [the shell](runtime.md#the-shell) describes; rounds do not lay text
out yet. `kosmo-text` depends on `kosmo-types`, `kosmo-render`, parley 0.13.0
and skrifa 0.49.0, and it is a core library on the deterministic path. The
SDK facade `kosmo-sdk` does not re-export it. The design rules that it
follows are in the [glyphs](../../architecture.md#glyphs),
[text stack](../../architecture.md#text-stack),
[fonts](../../architecture.md#fonts),
[numeric representation](../../architecture.md#numeric-representation) and
[determinism rules](../../architecture.md#determinism-rules) sections of the
architecture.

## Using it

`FontSet::new` takes the bytes of the two fonts. `FontSet::layout` lays a
paragraph out, `Paragraph::run` makes a glyph command of it,
`FontSet::prepare` fills the coverage masks of a display list, and
`FontSet::glyphs` hands them to the rasterizer. This is an example of the
crate documentation, which `cargo test` runs: it lays out `1 + 2 = 3` in Fira
Sans at 16 pixels per em and draws it in black.

```rust
use kosmo_render::{
    Color, ImageStore, ListBuilder, Rasterizer, RenderTarget, VelloRasterizer,
};
use kosmo_text::{FontSet, SANS, TextStyle};
use kosmo_types::{BufferDesc, Fx, PixelFormat, Point};

let sans = include_bytes!("../assets/fonts/firasans/FiraSans-Regular.ttf");
let mono = include_bytes!("../assets/fonts/firamono/FiraMono-Regular.ttf");
let mut set = FontSet::new(sans, mono).unwrap();
let style = TextStyle::new(SANS, Fx::from_units(16 * 64)).unwrap();
let paragraph = set.layout("1 + 2 = 3", style, None).unwrap();
assert_eq!(paragraph.lines().len(), 1);
let run = paragraph.run(Point::default(), Color::opaque(0, 0, 0)).unwrap();
let mut builder = ListBuilder::new();
builder.push(run).unwrap();
let list = builder.finish().unwrap();

let mut rasterizer = VelloRasterizer::new();
set.prepare(&list, &mut rasterizer).unwrap();
let desc = BufferDesc::new(80, 20, 320, PixelFormat::Rgba8Premul).unwrap();
let mut target = RenderTarget::new(desc).unwrap();
let whole = target.whole();
rasterizer
    .render(&list, &whole, set.glyphs(), &ImageStore::EMPTY, &mut target)
    .unwrap();
assert!(target.bytes().chunks(4).any(|pixel| pixel == [0, 0, 0, 255]));
```

The paths of `include_bytes!` are relative to `framework/text/src/lib.rs`.
Code in another crate of `framework/` takes the files of the same asset
directory; a backend gets them from whoever builds it, as the
[fonts](../../architecture.md#fonts) rules state.

## The fonts and the asset manifest

The asset directory `framework/text/assets/fonts/` holds each font in a
directory of its own, as the google/fonts repository does, beside its
original license file:

| File | Font | Role |
| --- | --- | --- |
| `firasans/FiraSans-Regular.ttf` | Fira Sans 4.203 | The proportional interface font, for labels, results and the status area |
| `firamono/FiraMono-Regular.ttf` | Fira Mono 3.206 | The monospaced font, for the optional style of the one-line input and for aligned columns of numbers |

Both are the static Regular styles from google/fonts at commit `29b4ec6`,
copied byte for byte, without subsetting, and distributed under the SIL Open
Font License 1.1 with their license files `OFL.txt`; the crate's license is
`MIT AND OFL-1.1`. The asset manifest `MANIFEST.toml` records for each font
its file, its size, its SHA-256 digest, its source and upstream commit, its
version, its role, its license and the path and digest of its license file.
The goldens step of `cargo xtask check` compares the directory with the
manifest and rejects a changed file, a file that the manifest does not list,
a third font and a font file outside the directory, as [the font
assets](../../../CONTRIBUTING.md#the-font-assets) describes. There is no
CJK font and no math font; adding, removing or replacing a font is a design
change.

`SANS_FILE` and `MONO_FILE` are the files of the two fonts in the manifest,
`"firasans/FiraSans-Regular.ttf"` and `"firamono/FiraMono-Regular.ttf"`,
which name them as resources of the platform interface.

## The font set

`FontSet::new(sans, mono)` copies the bytes of Fira Sans and Fira Mono and
registers only them with parley's font collection, fontique, in a collection
with system fonts off, without generic families and without fallback, so
text that the set lays out uses only these two fonts. `SANS` and `MONO` are
the `FontId`s of the two fonts, 0 and 1, and `FontSet::family_name` gives
their family names, `Fira Sans` and `Fira Mono`. `FontSet::new` refuses
bytes with a `FontSetError`:

| Error | When |
| --- | --- |
| `NotTrueType(font)` | The bytes of `font` are not one TrueType font: not a font file, a collection of fonts, no TrueType outlines, which skrifa looks for in the tables `glyf` and `loca`, or not registered as one font by fontique, which skips a font without a `name` table |
| `SameFamily` | Both fonts belong to the same family |

The font set does not check which fonts the bytes are, nor does it take
digests: the asset manifest and the tests of whoever builds a font set keep
its bytes those of the bundled files.

## Layout and line breaking

`TextStyle::new(font, size)` is the style of a paragraph: `SANS` or `MONO`
at a size in pixels per em, an `Fx` from `MIN_SIZE`, 1 pixel (64 units), to
`MAX_SIZE`, 256 pixels (16,384 units). `FontSet::layout(text, style, width)`
lays the text out with parley, breaking lines at `width`, an `Fx` from 0 to
`MAX_WIDTH`, $2^{18}$ pixels, or, with `None`, only at the line breaks of
the text:

- harfrust shapes the text, with the font's kerning and ligatures, and
  lines break by the Unicode line breaking algorithm with the data of
  icu_segmenter compiled into the program. A word longer than the width
  stays whole on a line of its own, and spaces at the end of a line hang
  beyond it.
- The paragraph runs from left to right whatever its first character. Only
  the style's font is used: a character that it lacks becomes the font's
  `.notdef` glyph.
- The size and the width reach parley as exact `f32` values, its alignment
  to pixels is off, and each position and length that it gives is
  multiplied by 64 and rounded once into an `Fx` by the rounding function of
  `kosmo-types`.

The `Paragraph` has the style, its width, the advance of its longest line
without the spaces that hang at the ends of lines, its height, the line
heights of the font added together, and its lines from the top, in the
coordinates of the paragraph, whose top-left corner is (0, 0). A `Line` has
the byte range of the text that it holds, with the spaces and the line break
at its end, its baseline from the top of the paragraph and its glyphs in
visual order, each a glyph identifier and an origin on the baseline. The
example of line breaking of the crate documentation, after it builds the font
set as above, breaks a sentence at 160 pixels:

```rust
let style = TextStyle::new(SANS, Fx::from_units(16 * 64)).unwrap();
let text = "The result is 1/3 and the error is 0";
let paragraph = set.layout(text, style, Some(Fx::from_units(160 * 64))).unwrap();
let lines: Vec<&str> = paragraph.lines().iter().map(|line| &text[line.text()]).collect();
assert_eq!(lines, ["The result is 1/3 and ", "the error is 0"]);
```

`Paragraph::run(origin, color)` makes the glyphs of every line one
`Command::Glyphs` of a display list, in the paragraph's font and size and in
`color`, each origin moved by `origin`. `TextError` reports a font other
than the two (`UnknownFont`), a size or a width out of range
(`SizeOutOfRange`, `WidthOutOfRange`) and a position, or an origin moved by
`run`, that does not fit in an `Fx` (`OutOfRange`).

## The glyph cache

Each font set holds a `GlyphCache` for its two fonts, built with the set and
dropped with it. Its key is the `GlyphKey` of `kosmo-render`: the font, the
size, the glyph identifier and the horizontal step of 0, 1/4, 1/2 or 3/4
pixel at which the rasterizer places the glyph. The `FontId` of a key stands
for the bytes of that font in this font set alone; a font set built from
other bytes starts with an empty cache of its own.

The cache holds at most `MAX_GLYPH_ENTRIES`, 4,096, entries, each step of a
glyph an entry of its own, and at most `MAX_GLYPH_BYTES`, 4 MiB (4,194,304
bytes), of coverage values, one byte for each pixel of a mask, the widths
times the heights of its masks added together. It counts its entries and
bytes itself, without the statistics of the global allocator.

`FontSet::prepare(list, rasterizer)` prepares the masks of every glyph run of
a display list, with the offsets of the groups that the run lies in, as the
next frame:

- Each preparation is a frame whose number is one greater than the last. It
  first marks every key of the list as used in the frame.
- A mask is the unhinted outline of the glyph that skrifa reads at the size,
  moved right by the step and filled through the rasterizer's
  `fill_coverage`, so dropping masks and filling them again gives the same
  bytes, and what is drawn does not depend on what the cache held before.
- When a new mask would bring the cache beyond either limit, the cache
  evicts entries that the frame does not use, in ascending order of the
  frame that last used them and then of their key, until the mask fits.
- A frame whose own masks exceed a limit is refused with
  `GlyphError::FrameTooLarge` before any mask is allocated: a frame with
  more keys than entries before any outline is read, and otherwise a frame
  whose masks exceed the bytes, counting the masks of its keys that the
  cache already holds and measuring the others from the bounds of the points
  of their outlines.

`prepare` returns a `Prepared` with the keys of the list, the masks that it
filled and the entries that it evicted. Its other errors are `UnknownFont`
and `SizeOutOfRange` for a run that the set cannot have laid out,
`NoOutline` for a glyph without an outline, `OutOfRange` for an origin or a
point that does not fit in an `Fx` or a mask larger than a buffer, and
`Render` for an error of the rasterizer; after any error the cache is as it
was. `FontSet::glyphs` gives the cache, read only, as the `GlyphCoverage` of
the list for `Rasterizer::render`; a glyph whose mask it does not hold is a
missing glyph there. `FontSet::clear_glyphs` drops every entry and keeps
the number of the frame. `entries`, `bytes`, `frame` and `contains` read the
state of the cache.

## The coverage of the interface strings

The interface strings are listed in
[`framework/text/tests/data/interface-strings.txt`](../../../framework/text/tests/data/interface-strings.txt),
grouped by the bundled font that shows them: the characters that the
interface may use, by Unicode block, and the strings that it shows. The
comments at the start of the file describe its format. The test
`tests/coverage.rs` of `kosmo-text` reads both fonts from the asset
directory and checks, with skrifa's character map, that the font of each
group maps every character of the strings of that group. If a font lacks a
character, the test fails and lists every missing character with its
string, its font and its code point, such as
`firasans/FiraSans-Regular.ttf: hole: U+25A1 '□'`.

| Group | Fira Sans | Fira Mono |
| --- | --- | --- |
| `ascii` | U+0020 to U+007E | U+0020 to U+007E |
| `latin-1` | U+00A0 to U+00FF | U+00A0 to U+00FF |
| `greek` | U+0391 to U+03A9 without U+03A2, and U+03B1 to U+03C9 | The same |
| `operators-and-arrows` | `−∕√∞∑∫∂∆≈≠≤≥⋅←↑→↓` | `−∕√∞∑∫∂≈≠≤≥←↑→↓` |

The Fira Sans group also holds the names of the kinds of errors that kalyx
reports, the annotation of a stale result, and the lifecycle states of an
application and the entries of the shell, whose wording the planned shell
fixes. The Fira Sans group holds 257 distinct characters and the Fira Mono
group 255.

A change that adds a string to the interface, or a character to a string,
also adds the string to the group of the font that shows it in this file,
in the same change. There is no fallback, so a character that the font
lacks would be drawn as the font's `.notdef` glyph. Fira Sans has no `□`
(U+25A1), so the formula editing control draws a hole as a rectangle. Fira
Mono has no `∆` (U+2206) and no `⋅` (U+22C5), so monospaced strings do not
use them. A string that needs a character that its font lacks is worded
without that character. Adding or replacing a font is a design change.

## Pixel goldens

The 13 pixel goldens of `kosmo-text` are in `framework/text/tests/golden/`,
one for each string whose glyphs `tests/shaping.rs` pins:

| Golden | Text |
| --- | --- |
| `text-ascii` | ASCII letters and punctuation |
| `text-digits` | Digits and ASCII operators |
| `text-kerning-ligatures` | Kerned pairs and the ligatures `fi`, `ffi` and `fl` |
| `text-operators` | The operators of the calculator, such as `−`, `×`, `÷` and `±` |
| `text-math-values` | Values with `√`, `≈`, `π`, `∞` and superscripts |
| `text-math-symbols` | Relations, sums, integrals, arrows and the division slash |
| `text-greek` | Greek letters |
| `text-combining` | Letters with combining marks beside precomposed ones |
| `mono-input` | One-line input in Fira Mono |
| `mono-operators` | Operators in Fira Mono |
| `mono-combining` | Combining marks in Fira Mono |
| `text-steps` | `Aa1` at each of the four steps |
| `text-sizes` | `Aa1` at 12, 24 and 32 pixels per em |

Each string is laid out at 16 pixels per em unless the table says
otherwise, without a width, prepared through `FontSet::prepare` and drawn by
`VelloRasterizer` in opaque black, `#000000ff`, on transparent black, into
a tightly packed `Rgba8Premul` canvas of its layout box rounded outwards to
whole pixels with a margin of 2 pixels on each side; `text-steps` and
`text-sizes` stack one paragraph for each step or size from the top. The
files are encoded as those of [rendering](rendering.md#pixel-goldens) are
and listed in `MANIFEST`, and a test compares each drawing with its file
byte for byte, without tolerance. The glyphs of the strings are pinned as
the text form of their display lists, and the 9 cases of line breaking of
`tests/line_breaking.rs` are pinned as the text of their lines, without
goldens.

These commands, from the repository root, write the goldens of
`kosmo-render` and `kosmo-text` and their manifests again, and draw them
again into `framework/target/golden/<package>/` and compare each digest with
`MANIFEST`, changing no tracked file:

```sh
cargo xtask golden
cargo xtask golden --verify
```

Goldens are written only by `cargo xtask golden`, the check on GitHub-hosted
runners is authoritative, and both commands, like `cargo xtask check`, fail
first when the environment variable `ICU4X_DATA_DIR` is set, since it
changes the Unicode data of line breaking, as [golden images and
determinism](../../../CONTRIBUTING.md#golden-images-and-determinism) states.

## Determinism and the platform boundary

`kosmo-text` is a core library with `#![forbid(unsafe_code)]`, `no_std`
with `alloc`. It takes the fonts only as bytes, reads no files and does not
depend on `kosmo-platform`, and its `src/` uses none of the interfaces that
the [platform boundary](../../architecture.md#platform-boundary) forbids,
which the layout step of `cargo xtask check` checks. Clippy rejects the hash
containers and the floating-point methods that [the determinism
lints](../../../CONTRIBUTING.md#the-determinism-lints) list in all its
targets, as in every crate of the product workspaces, and since the layout
table marks it as a crate of the deterministic path, the layout step rejects
any attribute in it that turns these lints off. The glyph cache keeps its
entries in ordered containers. parley and skrifa are built without
their default features and without `std`, with `libm`, so parley's system
font back end `system` is not built and the transcendental functions inside
them come from the same `libm` as those of `kosmo-render`. The deps step
compares their features with the allow entries of `kosmo-text` and the
allowlist of `framework/`, so a change that turns on `std` or `system`
fails until those entries record it. A test lays the strings out again in a
process of its own and compares the result with that of the first process,
which covers the hash containers inside the text stack.

## Limits

- There is no font fallback and no bidirectional text: a paragraph runs
  from left to right in one font, and a character that the font lacks is
  drawn as its `.notdef` glyph. Neither font has right-to-left letters;
  Fira Sans maps only the right-to-left mark U+200F, which shaping draws as
  a glyph without advance, and parley still orders a run of right-to-left
  characters by the Unicode bidirectional algorithm.
- parley's feature `complex-scripts`, which adds dictionary-based line
  breaking for scripts written without spaces between words, such as Thai
  or Chinese, is off; neither font has those scripts.
- The fonts are the two bundled ones only, at sizes from 1 to 256 pixels
  per em; there is no math font and no OpenType MATH layout.
- Glyphs are not hinted; vertically a glyph has one position, and
  horizontally four steps.
- parley's hit testing and caret geometry are not offered yet; they come
  with the controls that use them.
- Rounds do not lay text out: the shell's font set serves only the frames
  that its driver draws, and a driver that presents text lays it out with a
  font set of its own built from the same bytes, as [the
  shell](runtime.md#the-shell) describes.
- Each font set holds a copy of both fonts, 631,592 bytes, besides its glyph
  cache; the time of layout and of filling masks has not been measured.

## Requirements

The tests are in `framework/text`: the unit tests in the `tests` modules
under `src/` (named here with their module, such as `fonts::tests::…`), the
integration tests in `tests/` (named with their file), which share the
strings, the cases of line breaking and the helpers in `tests/support/`, and
the examples of the crate documentation; those of the checks are in
`xtask`, and those of the shell are in `framework/ui` and
`framework/backend-headless`, as [the framework
runtime](runtime.md#requirements) lists them.

1. The font set has only the two bundled families, refuses bytes that are
   not one TrueType font, a font without a `name` table and the same family
   twice, and its file names are those of the asset manifest. Tests:
   `fonts::tests::`
   `the_font_set_has_only_the_two_bundled_families`,
   `bytes_that_are_not_one_true_type_font_name_their_font`,
   `a_font_without_a_name_table_is_not_registered`,
   `the_same_family_twice_is_rejected`,
   `the_file_names_are_those_of_the_asset_manifest`.
2. Each of the 13 strings shapes into its pinned glyphs: ASCII, digits and
   operators, kerning and ligatures, the operators of the calculator,
   mathematical values and symbols, Greek letters, combining characters in
   both fonts, monospaced input and operators, the four steps and three
   sizes. Tests: `tests/shaping.rs`: `ascii_letters_and_punctuation`,
   `digits_and_ascii_operators`, `kerning_and_ligatures`,
   `operators_of_the_calculator`, `mathematical_values`,
   `mathematical_symbols`, `greek_letters`, `combining_characters`,
   `monospaced_input`, `monospaced_operators`,
   `monospaced_combining_characters`, `the_four_quarter_pixel_steps`,
   `three_sizes`, `every_string_is_pinned`.
3. Layout runs from left to right, places the first glyph at each step,
   shapes a combining acute accent as the precomposed letter, spaces
   monospaced glyphs 9.6 pixels apart, takes sizes between whole pixels
   exactly, and makes runs of the glyphs of every line in order with the
   font, size and color of the call, refusing one moved beyond the fixed
   point. Tests: `tests/shaping.rs`:
   `a_paragraph_runs_left_to_right_whatever_its_first_character`,
   `the_first_glyph_lies_at_each_step`,
   `a_combining_acute_accent_shapes_as_the_precomposed_letter`,
   `monospaced_glyphs_lie_9_6_pixels_apart`,
   `a_size_between_whole_pixels_reaches_parley_exactly`,
   `a_run_holds_the_glyphs_of_every_line_in_order`,
   `a_run_has_the_font_size_and_color_of_its_call`,
   `a_run_moved_beyond_the_fixed_point_is_refused`;
   `layout::tests::sizes_and_widths_reach_parley_exactly`.
4. The 9 cases of line breaking give their pinned lines: a break at a
   space, a line feed, a word longer than the width, a minus sign kept with
   its number, a no-break space, a combining mark kept with its letter,
   hanging spaces, no width and a fraction in Fira Mono; lines lie one line
   height apart, and widths outside 0 to `MAX_WIDTH` are refused. Tests:
   `tests/line_breaking.rs`: `a_line_breaks_at_a_space`,
   `a_line_feed_breaks_the_line`, `a_word_longer_than_the_width_stays_whole`,
   `a_minus_sign_stays_with_its_number`, `a_no_break_space_does_not_break`,
   `a_combining_mark_stays_with_its_letter`,
   `spaces_at_the_end_of_a_line_hang`, `without_a_width_the_text_is_one_line`,
   `a_fraction_does_not_break_after_its_slash`, `every_case_is_pinned`,
   `a_word_longer_than_the_width_overflows_it`,
   `lines_lie_one_line_height_apart`,
   `widths_outside_0_to_the_largest_are_refused`.
5. Each of the 13 strings matches its pixel golden byte for byte, on a
   canvas of its layout box with a margin that holds every glyph, and a
   drawing one byte off or of another canvas fails. Tests: `tests/golden.rs`:
   the 13 tests `ascii_letters_and_punctuation_match_their_golden` to
   `three_sizes_match_their_golden`,
   `each_golden_is_its_layout_box_rounded_outwards_with_a_margin`,
   `the_margin_holds_every_glyph_of_each_golden`,
   `a_golden_file_one_byte_off_its_drawing_fails`,
   `a_drawing_of_another_canvas_or_length_fails_with_both`,
   `the_helper_of_the_comparisons_fails_without_a_string`.
6. One font set lays the same text out alike twice, and another process
   lays it out alike. Tests: `tests/determinism.rs`:
   `one_font_set_lays_the_same_text_out_alike_twice`,
   `this_process_writes_its_layout`,
   `another_process_lays_the_same_text_out_alike`.
7. The glyph cache holds 4,096 entries and 4 MiB; a new key evicts exactly
   the key that the frame does not use, by the frame that last used it and
   then by key, for entries and for bytes; a frame at the capacity evicts
   nothing, and one beyond it is refused before its masks are allocated or
   filled; a drawing rebuilt after evictions and after clearing has the
   same pixels. Tests: `cache::tests::`
   `the_capacity_is_4096_entries_and_4_mib`,
   `a_new_key_evicts_the_one_key_that_the_frame_does_not_use`,
   `entries_are_evicted_by_the_frame_that_last_used_them_and_then_by_key`,
   `an_older_frame_goes_first_whatever_its_key`,
   `the_order_matches_the_entries_after_clearing`,
   `bytes_beyond_the_capacity_evict_too`,
   `held_masks_count_towards_a_frame_beyond_the_bytes`,
   `a_frame_at_the_capacity_evicts_nothing`,
   `a_mask_that_fills_the_bytes_exactly_evicts_nothing`,
   `a_frame_beyond_the_capacity_is_refused_before_filling`,
   `a_frame_beyond_the_capacity_is_refused_before_its_masks_are_allocated`,
   `masks_are_measured_from_the_bounds_of_their_points`,
   `a_drawing_rebuilt_after_evictions_has_the_same_pixels`,
   `clearing_keeps_the_number_of_the_frame`.
8. Keys take the steps of origins with the offsets of their groups; masks
   are the unhinted outlines at each step and at sizes between whole pixels;
   sizes and fonts outside the set, an error of a key and a mask that the
   rasterizer cannot fill leave the cache as it was. Tests:
   `cache::tests::`
   `keys_take_the_steps_of_origins_with_the_offsets_of_their_groups`,
   `keys_take_the_offsets_of_nested_groups_until_each_pops`,
   `masks_are_the_unhinted_outlines_at_each_step`,
   `a_mask_between_whole_pixels_is_drawn_at_its_exact_size`,
   `sizes_and_fonts_outside_the_set_are_refused`,
   `an_error_of_a_key_comes_before_any_fill_and_before_the_bytes`,
   `a_mask_that_the_rasterizer_cannot_fill_leaves_the_cache_as_it_was`.
9. The bundled fonts cover every interface string; a hole `□` in a string
   of Fira Sans fails the check, which lists every missing character once
   for each string; the file's format groups strings by font, reads
   escapes and rejects what it does not define. Tests: `tests/coverage.rs`:
   `the_bundled_fonts_cover_every_interface_string`,
   `a_hole_in_the_proportional_font_fails_the_check`,
   `the_check_lists_every_missing_character_once_for_each_string`,
   `the_format_groups_strings_by_font_and_reads_escapes`,
   `the_format_rejects_what_it_does_not_define`.
10. The goldens step passes the fonts of this repository, which are those
    of google/fonts, and reports a font or a license file changed by one
    byte, a file that the manifest does not list, a third font and a font
    outside the asset directory, among other problems. Tests in `xtask`:
    `assets::tests::`
    `the_font_assets_of_this_repository_pass`,
    `the_manifest_of_this_repository_lists_the_unmodified_fonts_of_google_fonts`,
    `the_bundled_fira_sans_changed_by_one_byte_is_reported_with_both_digests`,
    `a_changed_license_file_is_reported`,
    `a_file_that_the_manifest_does_not_list_is_reported`,
    `a_third_font_is_reported_even_when_the_manifest_lists_it`,
    `a_font_outside_the_asset_directory_is_reported`, and the other tests
    of `xtask/src/assets/tests.rs`.
11. The deps step reports parley's default features, which turn on its
    system font back end, against the allow entry of `kosmo-text`, and
    `cargo xtask check` and `cargo xtask golden` fail first when
    `ICU4X_DATA_DIR` is set. Tests in `xtask`:
    `deps::tests::the_allow_entry_detects_system_font_discovery_turned_on_for_the_text_stack`,
    `tests::check_and_golden_fail_first_when_icu_data_is_set`.

These commands, from the repository root, run the tests of the text crate,
draw its goldens again and compare their digests, and run the repository
checks:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-text
cargo xtask golden --verify
cargo xtask check
```
