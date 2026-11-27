//! Text of kosmo's framework: the two fonts that kosmo bundles, the shaping
//! and line breaking of paragraphs, and the glyph cache.
//!
//! The crate holds the two fonts in its asset directory `assets/fonts/`,
//! each in a directory of its own with its original license file:
//!
//! - `firasans/FiraSans-Regular.ttf`, Fira Sans 4.203, the proportional
//!   interface font for labels, results and the status area;
//! - `firamono/FiraMono-Regular.ttf`, Fira Mono 3.206, the monospaced font
//!   for the optional style of the one-line input and for aligned columns of
//!   numbers.
//!
//! Both are unmodified OFL-1.1 files, without subsetting, and keep their own
//! license. The asset manifest `assets/fonts/MANIFEST.toml` records the
//! size, the SHA-256 digest, the source and the license of each font and
//! the digest of its license file, and the goldens step of
//! `cargo xtask check` checks that the directory holds exactly the files
//! that it lists, unmodified.
//!
//! The crate takes the fonts only as bytes, which the framework obtains
//! through the platform interface, and reads no files. [`FontSet::new`]
//! takes the bytes of both fonts and registers only them with parley's font
//! collection, fontique, with system fonts off; it checks with skrifa that
//! each is one TrueType font. [`SANS`] and [`MONO`] are the fonts of the
//! set, and [`SANS_FILE`] and [`MONO_FILE`] their files in the asset
//! manifest, which name them as resources of the platform interface:
//!
//! ```
//! use kosmo_text::{FontSet, MONO, MONO_FILE, SANS, SANS_FILE};
//!
//! assert_eq!(SANS_FILE, "firasans/FiraSans-Regular.ttf");
//! assert_eq!(MONO_FILE, "firamono/FiraMono-Regular.ttf");
//! let sans = include_bytes!("../assets/fonts/firasans/FiraSans-Regular.ttf");
//! let mono = include_bytes!("../assets/fonts/firamono/FiraMono-Regular.ttf");
//! let mut set = FontSet::new(sans, mono).unwrap();
//! assert_eq!(set.family_name(SANS), Some("Fira Sans"));
//! assert_eq!(set.family_name(MONO), Some("Fira Mono"));
//! ```
//!
//! [`FontSet::layout`] lays out a paragraph with parley, in the
//! [`TextStyle`] of one of the two fonts at a size in 1/64 pixel: shaped by
//! harfrust and broken into lines by the Unicode line breaking algorithm,
//! with the data of icu_segmenter compiled in, from left to right, without
//! fallback and without bidirectional text. The [`Paragraph`] holds each
//! [`Line`] with the bytes of the text that it holds, its baseline and its
//! glyphs, every position rounded once from parley's `f32` into the 26.6
//! fixed point of `kosmo-types`, and [`Paragraph::run`] makes a glyph
//! command of a display list of them:
//!
//! ```
//! use kosmo_text::{FontSet, SANS, TextStyle};
//! use kosmo_types::Fx;
//!
//! let sans = include_bytes!("../assets/fonts/firasans/FiraSans-Regular.ttf");
//! let mono = include_bytes!("../assets/fonts/firamono/FiraMono-Regular.ttf");
//! let mut set = FontSet::new(sans, mono).unwrap();
//! let style = TextStyle::new(SANS, Fx::from_units(16 * 64)).unwrap();
//! let text = "The result is 1/3 and the error is 0";
//! let paragraph = set.layout(text, style, Some(Fx::from_units(160 * 64))).unwrap();
//! let lines: Vec<&str> = paragraph.lines().iter().map(|line| &text[line.text()]).collect();
//! assert_eq!(lines, ["The result is 1/3 and ", "the error is 0"]);
//! ```
//!
//! [`FontSet::prepare`] fills the coverage masks of the glyphs of a display
//! list through the rasterizer of `kosmo-render`, from the unhinted outlines
//! that skrifa reads, into the [`GlyphCache`] of the font set, which holds
//! at most [`MAX_GLYPH_ENTRIES`] masks of [`MAX_GLYPH_BYTES`] coverage
//! values, and [`FontSet::glyphs`] hands the cache to the rasterizer as the
//! glyph coverage of the list:
//!
//! ```
//! use kosmo_render::{
//!     Color, ImageStore, ListBuilder, Rasterizer, RenderTarget, VelloRasterizer,
//! };
//! use kosmo_text::{FontSet, SANS, TextStyle};
//! use kosmo_types::{BufferDesc, Fx, PixelFormat, Point};
//!
//! let sans = include_bytes!("../assets/fonts/firasans/FiraSans-Regular.ttf");
//! let mono = include_bytes!("../assets/fonts/firamono/FiraMono-Regular.ttf");
//! let mut set = FontSet::new(sans, mono).unwrap();
//! let style = TextStyle::new(SANS, Fx::from_units(16 * 64)).unwrap();
//! let paragraph = set.layout("1 + 2 = 3", style, None).unwrap();
//! assert_eq!(paragraph.lines().len(), 1);
//! let run = paragraph.run(Point::default(), Color::opaque(0, 0, 0)).unwrap();
//! let mut builder = ListBuilder::new();
//! builder.push(run).unwrap();
//! let list = builder.finish().unwrap();
//!
//! let mut rasterizer = VelloRasterizer::new();
//! set.prepare(&list, &mut rasterizer).unwrap();
//! let desc = BufferDesc::new(80, 20, 320, PixelFormat::Rgba8Premul).unwrap();
//! let mut target = RenderTarget::new(desc).unwrap();
//! let whole = target.whole();
//! rasterizer
//!     .render(&list, &whole, set.glyphs(), &ImageStore::EMPTY, &mut target)
//!     .unwrap();
//! assert!(target.bytes().chunks(4).any(|pixel| pixel == [0, 0, 0, 255]));
//! ```
//!
//! The crate is `no_std` with `alloc` and without unsafe code. It depends
//! on parley 0.13.0 and skrifa 0.49.0 without their default features and
//! without `std`, with `libm`: parley's system font back end `system` is
//! not built, and the transcendental functions inside both come from the
//! same `libm` as those of `kosmo-render`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod cache;
mod fonts;
mod layout;

pub use cache::{GlyphCache, GlyphError, MAX_GLYPH_BYTES, MAX_GLYPH_ENTRIES, Prepared};
pub use fonts::{FontSet, FontSetError, MONO, MONO_FILE, SANS, SANS_FILE};
pub use layout::{Line, MAX_SIZE, MAX_WIDTH, MIN_SIZE, Paragraph, TextError, TextStyle};
