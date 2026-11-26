//! Rendering of kosmo: display lists, and the wrapper of transcendental
//! functions for code that writes layout, display lists or pixels.
//!
//! - Display lists: a [`ListBuilder`] appends the [`Command`]s of a frame,
//!   with geometry in the 26.6 fixed point of `kosmo-types` and
//!   premultiplied [`Color`]s, and [`ListBuilder::finish`] gives the
//!   [`DisplayList`]. A list holds at most [`MAX_COMMANDS`] commands and
//!   [`MAX_ITEMS`] path elements and glyphs.
//! - [`det_math`], the wrapper through which code that writes layout,
//!   display lists or pixels calls transcendental functions.
//!
//! The renderer interface and the rasterizer on vello_cpu that draws
//! display lists are planned here.
//!
//! The crate is `no_std` with `alloc` and without unsafe code. Its
//! dependencies are built without `std`: vello_cpu with its `libm` and
//! `u8_pipeline` features, so that the transcendental functions inside the
//! raster library come from the same `libm` as those of [`det_math`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod det_math;
mod list;

pub use list::{
    Cap, Color, Command, DisplayList, FontId, Glyph, GlyphId, Join, ListBuilder, ListError,
    MAX_COMMANDS, MAX_ITEMS, Path, PathElement,
};
