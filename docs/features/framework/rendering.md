# Rendering

The library `kosmo-render` in the `framework/` workspace holds display lists
with their text form, the renderer interface `Rasterizer`, the rasterizer `VelloRasterizer`, which
draws display lists on the CPU with vello_cpu 0.4.0 on one thread, the kept
previous frame `RenderTarget` with its conversion into the buffer to be
presented, and `det_math`, the wrapper through which deterministic code calls
transcendental functions. The pixel formats, buffer descriptions, the
conversion of pixels between them and damage regions are in `kosmo-types` in
the `display/` workspace, which the framework's rendering and the display
server share. The same display list and damage region give the same bytes on
every run and every host, and pixel goldens check this.

The framework's drawing pass in `kosmo-ui` builds display lists, but rounds
do not call it yet, and nothing presents a frame: tests build display lists
and draw them. Text shaping and the glyph cache of `kosmo-text`, images and
presentation through the platform interface are planned; until they exist,
glyph coverage comes from the tests and the image store is empty.
`kosmo-render` depends on `kosmo-types`, vello_cpu and libm, and it is a core
library on the deterministic path. The design rules that it follows are in the
[renderer interface](../../architecture.md#renderer-interface),
[raster library](../../architecture.md#raster-library),
[pixel formats and buffers](../../architecture.md#pixel-formats-and-buffers),
[numeric representation](../../architecture.md#numeric-representation),
[transcendental functions](../../architecture.md#transcendental-functions)
and [determinism rules](../../architecture.md#determinism-rules) sections
of the architecture.

## Using it

A `ListBuilder` appends the commands of a frame and `finish` gives the
`DisplayList`. A `VelloRasterizer` draws the list into a `RenderTarget`, the
previous frame, inside a damage region. This is the example of the crate
documentation, which `cargo test` runs: it draws a red rectangle into a
4 by 3 frame, and then redraws only one pixel of it in blue.

```rust
use kosmo_render::{
    Color, Command, ImageStore, ListBuilder, NoGlyphs, Rasterizer, RenderTarget,
    VelloRasterizer,
};
use kosmo_types::{BufferDesc, Damage, Fx, PixelFormat, Point, Rect, Size};

let px = |pixels| Fx::from_pixels(pixels).unwrap();
let mut builder = ListBuilder::new();
let rect = Rect::new(Point::new(px(1), px(1)), Size::new(px(2), px(1)).unwrap());
builder.push(Command::FillRect { rect, color: Color::opaque(255, 0, 0) }).unwrap();
let list = builder.finish().unwrap();

let desc = BufferDesc::new(4, 3, 16, PixelFormat::Rgba8Premul).unwrap();
let mut target = RenderTarget::new(desc).unwrap();
let whole = target.whole();
let mut rasterizer = VelloRasterizer::new();
let work = rasterizer
    .render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut target)
    .unwrap();
assert_eq!(work.pixels, 12);
// Pixels (1, 1) and (2, 1) are opaque red; the others stay transparent.
assert_eq!(&target.bytes()[20..28], &[255, 0, 0, 255, 255, 0, 0, 255]);
assert_eq!(&target.bytes()[16..20], &[0, 0, 0, 0]);

// Only the damage changes: pixel (1, 1) becomes blue, (2, 1) stays red.
let mut builder = ListBuilder::new();
builder.push(Command::FillRect { rect, color: Color::opaque(0, 0, 255) }).unwrap();
let mut damage = Damage::new(desc);
damage.add(1, 1, 1, 1);
let list = builder.finish().unwrap();
rasterizer
    .render(&list, &damage, &NoGlyphs, &ImageStore::EMPTY, &mut target)
    .unwrap();
assert_eq!(&target.bytes()[20..28], &[0, 0, 255, 255, 255, 0, 0, 255]);
```

## Display lists

A display list is a sequence of commands. Geometry is in `Fx`, the 26.6
fixed point of `kosmo-types` in units of 1/64 pixel, and colors are `Color`
values, 8-bit sRGB-encoded red, green, blue and alpha, with the color
premultiplied by alpha; no field is a floating-point value or a bitmap, so a
display list holds no NaN and no infinity. The commands are:

| Command | Draws |
| --- | --- |
| `FillRect` | A rectangle in a color |
| `FillRoundedRect` | A rectangle with corners of a radius |
| `FillPath` | A path with the nonzero winding rule |
| `StrokePath` | A path stroked with a width, a `Join` (miter, round or bevel) and a `Cap` (butt, round or square) |
| `Glyphs` | A run of glyphs of one font and size, each a glyph identifier and an origin on the baseline |
| `PushClip`, `PopClip` | A group, whose commands are clipped to a rectangle and moved by an offset |

A `Path` starts with a move and holds straight, quadratic and cubic segments
and closes. `ListBuilder::push` checks each command when it is appended and
leaves the builder as it was when it refuses one, and `finish` checks that
every group has ended. A list holds at most `MAX_COMMANDS`, 16,384, commands
and at most `MAX_ITEMS`, 65,536, path elements and glyphs together; the
errors of `ListError` are `TooManyCommands`, `TooManyItems`,
`NegativeRadius`, `NegativeWidth`, `UnpairedPopClip` and `UnclosedClip`.
`Color::premultiplied` refuses a component greater than alpha.

## The text form of display lists

`DisplayList::text` writes a list as text, one line for each command in
drawing order, each followed by a line feed, so that records and tests
compare display lists byte for byte; an empty list has no text. Coordinates,
sizes, radii and widths are the integers of their `Fx`, in 1/64 pixel, and
colors are `#` and the 8 lowercase hexadecimal digits of the premultiplied
red, green, blue and alpha; no floating-point number is written. A point is
`<x>,<y>`, a size `<width>x<height>` and a rectangle
`<x>,<y> <width>x<height>`:

| Command | Line |
| --- | --- |
| `FillRect` | `fill-rect <color> <rect>` |
| `FillRoundedRect` | `fill-rounded-rect <color> <rect> radius <radius>` |
| `FillPath` | `fill-path <color>` and the elements of the path |
| `StrokePath` | `stroke-path <color> width <width> join <join> cap <cap>` and the elements of the path; the join is `miter`, `round` or `bevel`, the cap `butt`, `round` or `square` |
| `Glyphs` | `glyphs <color> font <font> size <size>` and ` <glyph>@<origin>` for each glyph, its number and its origin |
| `PushClip` | `push-clip <rect> offset <point>` |
| `PopClip` | `pop-clip` |

The elements of a path are ` M <point>`, ` L <point>`,
` Q <control> <point>`, ` C <first> <second> <point>` and ` Z`. For example,
a red rectangle of 3 by 4 pixels at (1, 2) inside a group moved by one pixel
to the right and half a pixel up:

```text
push-clip 64,128 192x256 offset 64,-32
fill-rect #ff0000ff 64,128 192x256
pop-clip
```

The text depends only on the list. A line holds at most 92 bytes, its line
feed included, and at most 74 more for each path element or glyph, so the
text of a list within its limits has at most
92 × 16,384 + 74 × 65,536 bytes, about 6.1 MiB.

## The renderer interface

The trait `Rasterizer` has two entries. `render` draws a display list into a
`RenderTarget`, which holds the previous frame, and writes only inside a
`Damage`, reading glyph coverage through a `GlyphCoverage` and images from an
`ImageStore`; it returns a `WorkCount`. `fill_coverage` fills an outline into
an 8-bit `CoverageMask` with the nonzero winding rule. The documentation of
the trait, in `framework/render/src/raster.rs`, states in one place the
pixel semantics of every command: the coordinates, with row 0 at the top;
coverage as the share of a pixel's area inside a shape; compositing with
source over on premultiplied 8-bit values; the edges of rectangles, rounded
corners, the nonzero rule, the joins and caps of strokes, the placement of
glyphs and their masks; and the order of clipping and moving in groups,
whose clips intersect.

For each rectangle of the damage region, in order, the pixels inside it start
as transparent black, the commands are drawn in order, and the rectangle is
written into the target; pixels outside every rectangle keep their bytes. A
pixel inside the region depends neither on the previous frame nor on the
other rectangles, so redrawing only the damage over the previous frame gives
the bytes of a whole redraw. `WorkCount` counts the rectangles, the commands
drawn once per rectangle and the pixels written; it depends only on the list
and the region, so it is the same on every host and for every rasterizer.

Before it writes anything, `render` checks its input and returns a
`RenderError`, leaving the target as it was:

| Error | When |
| --- | --- |
| `DamageMismatch` | The damage region is not a region of a buffer of the target's size |
| `MissingGlyph` | The glyph coverage has no mask for a glyph of the list, wherever the glyph lies |
| `Unsupported` | The rasterizer does not draw a command of the list |
| `OutOfRange` | An origin, or the offsets of nested groups, added together do not fit in an `Fx` |
| `OverLimit` | The input is beyond a limit of the rasterizer, such as an outline of more than `MAX_ITEMS` elements for `fill_coverage` |

A glyph's origin, with the offsets of its groups added, is placed by
`place_glyph`: `x` to the nearest quarter pixel, as a whole pixel and a
`Step` of 0, 1/4, 1/2 or 3/4, and `y` to the nearest whole pixel. A
`GlyphCoverage` gives the mask of a `GlyphKey`, the font, size, glyph and
step; `NoGlyphs` has no masks, for lists without glyphs.

Two implementations of the trait are tested against each other:
`VelloRasterizer` and the per-pixel reference rasterizer in
`framework/render/tests/support/reference.rs`. The reference decides each
pixel from its center, so it draws only what that decides exactly, the
commands of its fixed scenes: opaque rectangles and clips with whole-pixel
edges and offsets, and glyphs at the whole-pixel step with coverage of only
0 and 255; it reports every other command, and filling coverage, as
unsupported.

## The rasterizer on vello_cpu

`VelloRasterizer` draws with vello_cpu's `u8` pipeline on the calling thread,
with the SIMD level given explicitly as the baseline of the compilation
target, Neon on the AArch64 target platforms, never detected at run time.
vello_cpu is built without its default features and with `libm` and
`u8_pipeline`, so it reads no fonts, starts no threads and takes its
transcendental functions from the same `libm` as `det_math`. A call records
the whole display list once in a render context of the target's size and
rasterizes each damage rectangle as a cut-out of it, which gives every pixel
the bytes of a whole redraw; glyph masks are drawn as images, tinted with the
command's color through their coverage. A reused rasterizer gives the same
bytes as a new one. No type of vello_cpu appears in the interface of
`kosmo-render`.

## The previous frame and `convert_into`

`RenderTarget` is the frame that rendering keeps: a tightly packed
`Rgba8Premul` buffer, its stride four times its width, within the buffer
limits. `RenderTarget::new` makes one whose bytes are all 0, `from_bytes`
takes bytes of its description's length, and `whole` gives the damage region
of the whole buffer. Before presentation, `convert_into` writes each rectangle
of a damage region into the buffer to be presented, of any of the four
formats, at the same place, through the conversion function of
`kosmo-types`; every other byte of that buffer stays as it was. When the
buffer's bytes are not as many as its description says, or a rectangle lies
outside either buffer, it writes nothing and returns the `ConvertError`.
Repainting by buffer age for double buffering is done on the presenting
side, which converts any rectangle again.

## Pixel formats, buffers and damage regions

`kosmo-types` has the four pixel formats of `PixelFormat`:

| Format | Bytes per pixel | Layout |
| --- | --- | --- |
| `Rgba8Premul` | 4 | Bytes R, G, B, A in memory, premultiplied; the format that the rasterizer draws |
| `Xrgb8888` | 4 | Bytes B, G, R, X in memory, X written as `0xFF` |
| `Argb8888Premul` | 4 | Bytes B, G, R, A in memory, premultiplied |
| `Rgb565` | 2 | A little-endian 16-bit word with R in bits 15 to 11, G in bits 10 to 5 and B in bits 4 to 0 |

`BufferDesc::new(width, height, stride, format)` describes a buffer, its
stride in bytes; the first byte of pixel (x, y) is at
`y * stride + x * bpp`. It returns a `BufferError`: `Empty` for a width or
height of 0, `SideTooLong` beyond `MAX_BUFFER_SIDE`, 8,192 pixels,
`StrideTooShort` for a stride shorter than a row, and `TooManyBytes` when the
stride times the height is beyond `MAX_BUFFER_BYTES`, $2^{26}$ bytes
(64 MiB). The previous frame and coverage masks stay within the same limits.

`convert` writes a rectangle of a source buffer into a target buffer at a
place, for each of the 12 ordered pairs of different formats and as a copy
of rows within one format, with integer arithmetic only and without
dithering, as the [pixel formats and
buffers](../../architecture.md#pixel-formats-and-buffers) rules state; it
never allocates, and it writes nothing when the bytes are not as many as
their descriptions say or the rectangle lies outside either buffer.

A `Damage` is the damage region of one buffer: at most `MAX_DAMAGE_RECTS`,
16, `DamageRect`s in the order they were added. `Damage::add` drops a
rectangle without width or height or outside the buffer and clips one that
crosses its edge; the rectangle after the sixteenth that it keeps replaces
all of them, with itself, by their bounding rectangle. Regions of buffers
of other sizes are never equal.

## The wrapper of transcendental functions

`det_math` has `sin` for `f64` and `sinf` for `f32`, each forwarding to
`libm` 0.2.16 without an approximation of its own, so each gives the same
result on every host; a function is added with its first caller. It is the
one module of the repository that calls `libm`: everywhere else Clippy
rejects calls of `libm`, of the methods of `f32` and `f64` whose precision
is unspecified, of the `algebraic_*` methods and of floating-point `min`,
`max` and `clamp`, as [the determinism
lints](../../../CONTRIBUTING.md#the-determinism-lints) describe.

## Pixel goldens

The six pixel goldens of `kosmo-render` are in `framework/render/tests/golden/`,
one for each kind of command, each drawn by `VelloRasterizer` on a canvas of
64 by 48 pixels: `fill-rect`, `fill-rounded-rect`, `fill-path`,
`stroke-path`, `glyphs` and `clip-group`. The file `<name>.px` of a golden
holds the drawn buffer encoded as the [digests
rule](../../architecture.md#determinism-rules) states: the description in
16 bytes, the width, the height, the stride and the format code, each a
little-endian `u32`, with the codes 1 for `Rgba8Premul`, 2 for `Xrgb8888`, 3
for `Rgb565` and 4 for `Argb8888Premul`, followed by the raw bytes, without
image encoding. `MANIFEST` lists the SHA-256 of each file. A test compares a
drawing with its golden byte for byte, without tolerance, and a failure names
the first pixel or byte that differs.

These commands, from the repository root, write the goldens and their
manifest again, and draw them again into
`framework/target/golden/kosmo-render/` and compare each digest with
`MANIFEST`, changing no tracked file:

```sh
cargo xtask golden
cargo xtask golden --verify
```

The goldens step of `cargo xtask check` reports a golden whose file differs
from `MANIFEST`, a golden that no test refers to and a file that `MANIFEST`
does not list, among other problems, as [the goldens
step](../../../CONTRIBUTING.md#the-goldens-step-and-cargo-xtask-golden)
describes. Goldens are written only by `cargo xtask golden`, the check on
GitHub-hosted runners is authoritative, and a local digest that differs from
CI's stops the work, as [golden images and
determinism](../../../CONTRIBUTING.md#golden-images-and-determinism) states.

## Determinism and the platform boundary

`kosmo-render` and `kosmo-types` are core libraries with
`#![forbid(unsafe_code)]`, `no_std` with `alloc`; `kosmo-types` is pure core.
Their `src/` uses none of the interfaces that the [platform
boundary](../../architecture.md#platform-boundary) forbids, among them
threads, clocks, the environment, files and input and output, which the
layout step of `cargo xtask check` checks. Clippy rejects the hash containers
and the floating-point methods that the determinism lints list in all their
targets, as in every crate of the product workspaces, and since the layout
table marks both, and `kosmo-ui`, as crates of the deterministic path, the
layout step rejects any attribute in them that turns these lints off, except
the one of `det_math`; an attribute that a macro assembles from parts or
brings in from another crate is not found, as [the layout
step](../../../CONTRIBUTING.md#the-layout-step) states, and review reads
those macros. The deps step
compares the features of vello_cpu and the packages it brings in with the
allow entries of `kosmo-render` and the allowlist of `framework/`, so a change
that turns on `std` or `multithreading` fails until those entries record it.

## Limits

- Working memory: besides the target, `render` allocates a tightly packed
  `Rgba8Premul` buffer of its largest damage rectangle, 4 bytes per pixel,
  and an RGBA copy of each distinct glyph mask that the list uses, 4 bytes
  per value; `fill_coverage` allocates 4 bytes per value of the mask, at
  most 4 × `MAX_BUFFER_BYTES`. The rasterizer keeps its largest buffer until
  it is dropped, and the copies of the glyph masks stay in its render
  context until the next call.
- Every call of `render` records the whole display list, whatever the size of
  the damage region; the time of a frame has not been measured.
- Display lists have no image commands, gradients or blend modes other than
  source over, and the image store holds no images.
- Glyph coverage comes from the caller: the glyph cache of `kosmo-text`, which
  will fill masks through `fill_coverage`, is planned.
- Nothing presents a frame yet: the drawing pass of `kosmo-ui` builds
  display lists that rounds do not draw, and the platform interface is
  planned.
- The reference rasterizer draws only the commands of its fixed scenes.
- Goldens are drawn and compared on the AArch64 target platforms; on another
  architecture the SIMD level is that target's baseline.
- A damage region holds at most 16 rectangles; more merge into their bounding
  rectangle.

## Requirements

The tests are in `framework/render`: the unit tests in the `tests` modules
under `src/` (named here with their module, such as `list::tests::…`), the
integration tests in `tests/` (named with their file), which share the scenes
and helpers in `tests/support/`, and the examples of the crate
documentation; the tests of pixel formats, conversion and damage regions are
in `display/types`, and those of the checks in `xtask`.

1. Each kind of command has a pixel golden, which its drawing matches byte for
   byte, and the encoding is the description followed by the bytes. Tests:
   `tests/golden.rs`: `rectangles_match_their_golden`,
   `rounded_rectangles_match_their_golden`,
   `filled_paths_match_their_golden`, `stroked_paths_match_their_golden`,
   `glyph_runs_match_their_golden`, `clip_groups_match_their_golden`,
   `the_encoding_is_the_description_and_the_bytes`.
2. A drawing one byte off its golden, a golden file one byte off its drawing,
   a drawing of another buffer or length and an unknown format code fail and
   name the difference. Tests: `tests/golden.rs`:
   `a_drawing_one_byte_off_its_golden_fails_at_the_first_differing_pixel`,
   `a_golden_file_one_byte_off_its_drawing_fails`,
   `a_drawing_of_another_buffer_or_length_fails_with_both`,
   `a_drawing_with_an_unknown_format_code_fails_with_the_code`,
   `a_difference_after_the_pixels_of_a_row_names_the_byte`,
   `the_helper_of_the_comparisons_fails_without_a_scene`.
3. Drawing the same display list twice gives the same bytes, with a new and
   with a reused rasterizer, for each scene of one kind of command and for a
   composite scene of 128 by 96 pixels. Test: `tests/repeatable.rs`:
   `a_new_and_a_reused_rasterizer_draw_every_scene_alike`.
4. Redrawing only the damage over the previous frame gives the bytes of a
   whole redraw, for 1,000 random damage regions of 1 to 16 rectangles from a
   fixed seed over the composite scene. Tests: `tests/partial_redraw.rs`:
   `redrawing_random_damage_over_the_previous_frame_equals_a_whole_redraw`,
   `split_mix_64_gives_its_published_sequence`.
5. The reference rasterizer and `VelloRasterizer` give the same bytes and work
   count for six fixed scenes, and the reference reports other commands as
   unsupported. Tests: `tests/reference_scenes.rs`:
   `overlapping_opaque_rectangles_are_drawn_in_order`,
   `rectangles_beyond_the_edges_are_clipped_and_empty_ones_draw_nothing`,
   `nested_clips_intersect`, `a_scrolled_group_moves_its_content_out_of_its_clip`,
   `only_the_damage_is_drawn_over_the_previous_frame`,
   `glyphs_with_coverage_of_0_and_255_at_whole_pixels_are_drawn_alike`,
   `the_reference_reports_other_commands_as_unsupported`.
6. A display list refuses the command after its 16,384th and a command that
   would bring it beyond 65,536 items, and stays as it was; it refuses a
   negative radius or width, a group ended twice or left open, and a color
   component beyond alpha. Tests: `tests/bounds.rs`:
   `the_command_after_the_last_allowed_one_is_refused_and_the_list_stays`,
   `a_command_beyond_the_items_is_refused_and_the_list_stays`; `list::tests::`
   `a_negative_radius_or_width_is_refused`, `a_pop_without_a_group_is_refused`,
   `a_list_with_an_open_group_cannot_be_finished`,
   `paths_and_glyphs_count_as_items`,
   `a_color_component_beyond_alpha_is_refused`.
7. `render` refuses a damage region of a buffer of another size, a missing
   glyph wherever it lies, and an origin or group offsets that do not fit,
   before it writes anything; `fill_coverage` refuses an outline beyond the
   items. Tests: `tests/render_errors.rs`:
   `a_damage_region_of_a_buffer_of_another_size_is_refused`,
   `a_missing_glyph_is_refused_even_outside_the_damage`,
   `an_origin_that_does_not_fit_with_its_offsets_is_refused`,
   `offsets_of_nested_groups_that_do_not_fit_are_refused`,
   `an_outline_beyond_the_items_is_over_the_limit`.
8. Strokes, glyphs and groups follow the pixel semantics: a miter join beyond
   4 half widths is a bevel join, a stroke without width covers nothing,
   glyph origins go to the nearest quarter and whole pixel, a glyph in a
   moved group lies at its origin with the offset, and the glyph scene has a
   glyph at each quarter step. Tests: `tests/strokes.rs`:
   `a_miter_join_beyond_4_half_widths_is_a_bevel_join`,
   `a_stroke_without_width_covers_nothing`;
   `raster::tests::glyph_origins_go_to_the_nearest_quarter_and_whole_pixel`;
   `tests/groups.rs`: `a_glyph_in_a_moved_group_lies_at_its_origin_with_the_offset`;
   `tests/repeatable.rs`: `the_glyph_scene_places_one_glyph_at_each_quarter_step`.
9. The previous frame is a tightly packed `Rgba8Premul` buffer that accepts
   only regions of buffers of its size; `convert_into` converts the damage
   into the buffer to be presented and writes nothing when it cannot; the
   work count depends on the list and the region only; a coverage mask is
   bounded like a buffer. Tests: `raster::tests::`
   `a_target_is_a_tightly_packed_rgba8_premul_buffer`,
   `a_target_accepts_only_regions_of_buffers_of_its_size`,
   `a_target_from_bytes_needs_the_length_of_its_description`,
   `the_damage_is_converted_into_the_buffer_to_present`,
   `a_conversion_that_cannot_be_done_writes_nothing`,
   `the_work_count_depends_on_the_list_and_the_region_only`,
   `a_mask_is_bounded_like_a_buffer`.
10. The SIMD level is the baseline of the target, Neon on AArch64, and every
    premultiplied color reaches vello_cpu unchanged. Tests: `vello::tests::`
    `the_simd_level_is_neon_on_aarch64`,
    `the_simd_level_is_the_baseline_of_the_target`,
    `every_premultiplied_color_reaches_vello_cpu_unchanged`.
11. Buffer descriptions reject an empty buffer, a side beyond 8,192 pixels, a
    short stride and more than $2^{26}$ bytes, among them a 4,097 by 4,097
    `Rgba8Premul` buffer. Tests in `display/types`: `pixel::tests::`
    `an_empty_buffer_is_rejected`, `a_side_longer_than_the_limit_is_rejected`,
    `a_stride_shorter_than_a_row_is_rejected`,
    `the_bytes_are_limited_by_the_stride_times_the_height`,
    `the_first_check_that_fails_is_the_error`,
    `rgb565_has_two_bytes_per_pixel_and_the_others_four`,
    `a_description_keeps_its_size_stride_and_format`;
    `tests/buffer_limits.rs`:
    `a_4097_by_4097_rgba8_premul_buffer_is_rejected_as_too_many_bytes`.
12. The conversion function covers the 12 ordered pairs of formats and copies
    rows within one format, every 5-bit and 6-bit value comes back unchanged,
    8-bit values narrow and widen to the nearest value, only the rectangle is
    written, and a conversion that cannot be done writes nothing. Tests in
    `display/types`: the 26 tests of `convert::tests`, among them
    `rgba8_premul_to_rgb565`, `every_rgb565_pixel_comes_back_unchanged_through_rgba8_premul`,
    `every_8_bit_value_is_narrowed_to_the_nearest_5_bit_and_6_bit_value`,
    `only_the_rectangle_is_written_at_its_place_in_the_target` and
    `a_rectangle_outside_either_buffer_is_rejected_and_writes_nothing`.
13. Damage regions drop empty rectangles and those outside the buffer, clip
    the others, keep their order, and merge into the bounding rectangle after
    16. Tests in `display/types`: `damage::tests::`
    `a_rectangle_without_width_or_height_is_dropped`,
    `a_rectangle_is_clipped_to_the_buffer`,
    `a_rectangle_outside_the_buffer_is_dropped`,
    `rectangles_keep_the_order_they_were_added_in`,
    `the_rectangle_after_the_limit_merges_all_into_their_bounds`,
    `regions_of_buffers_of_another_size_are_not_equal`,
    `the_same_rectangles_in_the_same_order_give_the_same_region`.
14. The text form writes each kind of command, each join and cap and each
    kind of path element as its line states, writes nothing for an empty
    list, and keeps each line within its bound for the widest values.
    Tests: `text::tests::every_command_kind_has_its_line`,
    `text::tests::an_empty_list_has_no_text`,
    `text::tests::lines_stay_within_their_bound_at_the_extremes`, and the
    example of `DisplayList::text`.
15. `det_math` gives the results of `libm`. Tests: `det_math::tests::`
    `sin_of_one_is_libm_sin`, `sinf_of_one_is_libm_sinf`.
16. Clippy, with the root `clippy.toml`, reports a method call, a path call and
    a function pointer of a method with unspecified precision,
    `algebraic_add`, the maximum of two `f32` values, `libm` outside the
    wrapper module and a `HashMap` of the standard library, also through a
    re-export, and not the `sin` method of an own type; the layout step
    rejects an attribute that turns these lints off in a crate of the
    deterministic path outside the wrapper module, and a `clippy.toml` beside
    the root one. Tests in `xtask`: `layout::lints::fixture::`
    `clippy_reports_a_method_call_of_f32_sin`,
    `clippy_reports_a_path_call_of_f64_powf`,
    `clippy_reports_f32_exp_as_a_function_pointer`,
    `clippy_reports_algebraic_add`, `clippy_reports_the_maximum_of_two_f32`,
    `clippy_reports_libm_outside_the_wrapper_module`,
    `clippy_does_not_report_the_sin_method_of_an_own_type`,
    `clippy_reports_a_std_hash_map`,
    `clippy_reports_a_type_through_its_re_export`,
    `clippy_reports_every_listed_method_function_and_type`;
    `layout::tests::rejects_an_allowance_of_the_methods_lint_outside_the_wrapper_module`,
    `layout::tests::rejects_a_clippy_configuration_beside_the_root_one`.
17. The deps step reports `std` resolved for a package inside the raster
    library against the allowlist, and `multithreading` resolved for
    vello_cpu against the allow entry of `kosmo-render`. Tests in `xtask`:
    `deps::tests::the_allowlist_detects_std_turned_on_inside_the_raster_library`,
    `deps::tests::the_allow_entry_detects_multithreading_turned_on_for_the_raster_library`.
18. The goldens step reports a golden edited by hand, a golden that no test
    refers to and a manifest that differs from the files. Tests in `xtask`:
    `golden::tests::a_hand_edited_golden_is_reported`,
    `golden::tests::a_golden_that_no_test_references_is_reported`,
    `golden::tests::a_file_that_the_manifest_lacks_is_reported`,
    `golden::tests::a_golden_without_a_file_is_reported`.

These commands, from the repository root, run the tests of rendering and of
the shared types, and the repository checks:

```sh
cargo test --locked --manifest-path framework/Cargo.toml -p kosmo-render
cargo test --locked --manifest-path display/Cargo.toml -p kosmo-types
cargo xtask check
```
