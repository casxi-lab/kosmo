//! Rendering of kosmo: display lists, the renderer interface and the
//! rasterization of display lists on the CPU with vello_cpu, on one
//! thread.
//!
//! - Display lists: a [`ListBuilder`] appends the [`Command`]s of a frame,
//!   with geometry in the 26.6 fixed point of `kosmo-types` and
//!   premultiplied [`Color`]s, and [`ListBuilder::finish`] gives the
//!   [`DisplayList`]. A list holds at most [`MAX_COMMANDS`] commands and
//!   [`MAX_ITEMS`] path elements and glyphs. [`DisplayList::text`] writes
//!   a list as text, one command per line, without floating-point numbers.
//! - The renderer interface: the [`Rasterizer`] trait, whose documentation
//!   states what every command draws. It draws a display list into the
//!   kept previous frame, a [`RenderTarget`] in tightly packed
//!   `Rgba8Premul`, only inside a damage region, with glyph coverage from a
//!   [`GlyphCoverage`], and fills outlines into [`CoverageMask`]s.
//!   [`RenderTarget::convert_into`] converts the damage into the buffer to
//!   be presented.
//! - [`VelloRasterizer`], the rasterizer on vello_cpu 0.4.0. No type of
//!   vello_cpu appears in the interface of this crate.
//! - [`det_math`], the wrapper through which code that writes layout,
//!   display lists or pixels calls transcendental functions.
//!
//! ```
//! use kosmo_render::{
//!     Color, Command, ImageStore, ListBuilder, NoGlyphs, Rasterizer, RenderTarget,
//!     VelloRasterizer,
//! };
//! use kosmo_types::{BufferDesc, Damage, Fx, PixelFormat, Point, Rect, Size};
//!
//! let px = |pixels| Fx::from_pixels(pixels).unwrap();
//! let mut builder = ListBuilder::new();
//! let rect = Rect::new(Point::new(px(1), px(1)), Size::new(px(2), px(1)).unwrap());
//! builder.push(Command::FillRect { rect, color: Color::opaque(255, 0, 0) }).unwrap();
//! let list = builder.finish().unwrap();
//!
//! let desc = BufferDesc::new(4, 3, 16, PixelFormat::Rgba8Premul).unwrap();
//! let mut target = RenderTarget::new(desc).unwrap();
//! let whole = target.whole();
//! let mut rasterizer = VelloRasterizer::new();
//! let work = rasterizer
//!     .render(&list, &whole, &NoGlyphs, &ImageStore::EMPTY, &mut target)
//!     .unwrap();
//! assert_eq!(work.pixels, 12);
//! // Pixels (1, 1) and (2, 1) are opaque red; the others stay transparent.
//! assert_eq!(&target.bytes()[20..28], &[255, 0, 0, 255, 255, 0, 0, 255]);
//! assert_eq!(&target.bytes()[16..20], &[0, 0, 0, 0]);
//!
//! // Only the damage changes: pixel (1, 1) becomes blue, (2, 1) stays red.
//! let mut builder = ListBuilder::new();
//! builder.push(Command::FillRect { rect, color: Color::opaque(0, 0, 255) }).unwrap();
//! let mut damage = Damage::new(desc);
//! damage.add(1, 1, 1, 1);
//! let list = builder.finish().unwrap();
//! rasterizer
//!     .render(&list, &damage, &NoGlyphs, &ImageStore::EMPTY, &mut target)
//!     .unwrap();
//! assert_eq!(&target.bytes()[20..28], &[0, 0, 255, 255, 255, 0, 0, 255]);
//! ```
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
mod raster;
mod text;
mod vello;

pub use list::{
    Cap, Color, Command, DisplayList, FontId, Glyph, GlyphId, Join, ListBuilder, ListError,
    MAX_COMMANDS, MAX_ITEMS, Path, PathElement,
};
pub use raster::{
    CoverageMask, GlyphCoverage, GlyphKey, ImageStore, NoGlyphs, Placement, Rasterizer,
    RenderError, RenderTarget, Step, WorkCount, place_glyph,
};
pub use vello::VelloRasterizer;
