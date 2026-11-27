//! Text of kosmo's framework.
//!
//! So far the crate holds the two fonts that kosmo bundles, in its asset
//! directory `assets/fonts/`, each in a directory of its own with its
//! original license file:
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
//! Shaping and line breaking with parley, glyph outlines through skrifa and
//! the glyph cache, which gives the renderer of `kosmo-render` the coverage
//! of glyphs, are planned in this crate.
//!
//! The crate is `no_std` with `alloc` and without unsafe code. It depends
//! on parley 0.13.0 and skrifa 0.49.0 without their default features and
//! without `std`, with `libm`: parley's system font back end `system` is
//! not built, and the transcendental functions inside both come from the
//! same `libm` as those of `kosmo-render`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod fonts;

pub use fonts::{FontSet, FontSetError, MONO, MONO_FILE, SANS, SANS_FILE};
