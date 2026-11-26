//! Rendering of kosmo: the renderer interface, display lists and their
//! rasterization on the CPU with vello_cpu, on one thread.
//!
//! So far the crate holds [`det_math`], the wrapper through which code
//! that writes layout, display lists or pixels calls transcendental
//! functions. The renderer interface, display lists, the rasterizer and
//! the previous frame are planned here.
//!
//! The crate is `no_std` with `alloc` and without unsafe code. Its
//! dependencies are built without `std`: vello_cpu with its `libm` and
//! `u8_pipeline` features, so that the transcendental functions inside the
//! raster library come from the same `libm` as those of [`det_math`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod det_math;
