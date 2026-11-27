//! Pixel goldens of the pinned strings: the canvas of each string, its
//! drawing through the glyph cache of the font set, the encoding of the
//! drawn buffer, the comparison of a drawing with its golden and the writer
//! of the goldens.
//!
//! The golden `<name>` of a string of [`STRINGS`] is the file
//! `tests/golden/<name>.px`, encoded as the goldens of `kosmo-render` are:
//! the description of the buffer in 16 bytes, its width, height, stride and
//! format code as little-endian `u32`s, followed by the bytes of the buffer.
//! `cargo xtask golden` runs the writers and lists the digests of the files
//! in `tests/golden/MANIFEST`; the other tests only read the goldens.
//!
//! The canvas of a string holds one paragraph for each of its sizes and, at
//! each size, each of its offsets, in that order, laid out without a width:
//! each paragraph lies below the one before, with its origin moved right by
//! its offset. The layout box is the union of the boxes of the paragraphs,
//! each from its origin to its width and height, rounded outwards to whole
//! pixels, and the canvas adds a margin of [`MARGIN`] pixels on each side.
//! It is a tightly packed `Rgba8Premul` buffer of transparent black, into
//! which vello_cpu draws the string in opaque black, from the coverage masks
//! that the glyph cache of the font set fills.

use std::fs;

use kosmo_render::{
    DisplayList, ImageStore, ListBuilder, Rasterizer, RenderTarget, VelloRasterizer,
};
use kosmo_text::FontSet;
use kosmo_types::{BufferDesc, Fx, PixelFormat, Point};

use super::{BLACK, Checked, STRINGS, Shaped, failed, font_set, lay_out};

/// The directory of the goldens.
pub const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

/// The directory, in the workspace's default build directory `target/`,
/// into which `cargo xtask golden --verify` draws the goldens again.
pub const REDRAWN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/golden/kosmo-text");

/// The margin of a canvas around the layout box, in pixels.
pub const MARGIN: i32 = 2;

/// The bytes of the encoded description.
pub const HEADER: usize = 16;

/// The format code of `Rgba8Premul` in the encoding.
const RGBA8_PREMUL: u32 = 1;

/// The display list of `shaped` on its canvas with a margin of `margin`
/// pixels, and the description of the canvas.
pub fn scene(
    set: &mut FontSet,
    shaped: &Shaped,
    margin: i32,
) -> Checked<(DisplayList, BufferDesc)> {
    let inset = Fx::from_pixels(margin).ok_or("no margin")?;
    let mut placed = Vec::new();
    let (mut top, mut right) = (Fx::ZERO, Fx::ZERO);
    for &size in shaped.sizes {
        let paragraph = lay_out(set, shaped.text, shaped.font, size, None)?;
        for &offset in shaped.offsets {
            let offset = Fx::from_units(offset);
            let end = offset.checked_add(paragraph.width()).ok_or("too wide")?;
            right = right.max(end);
            placed.push((paragraph.clone(), offset, top));
            top = top.checked_add(paragraph.height()).ok_or("too high")?;
        }
    }
    let mut builder = ListBuilder::new();
    for (paragraph, x, y) in &placed {
        let origin = Point::new(*x, *y)
            .translated(inset, inset)
            .ok_or("no origin")?;
        let run = paragraph.run(origin, BLACK).map_err(failed)?;
        builder.push(run).map_err(failed)?;
    }
    let list = builder.finish().map_err(failed)?;
    let width = outwards(right) + 2 * margin;
    let height = outwards(top) + 2 * margin;
    let (width, height) = (pixels(width)?, pixels(height)?);
    let desc =
        BufferDesc::new(width, height, width * 4, PixelFormat::Rgba8Premul).map_err(failed)?;
    Ok((list, desc))
}

/// `length`, which starts at 0, rounded outwards to whole pixels.
fn outwards(length: Fx) -> i32 {
    (length.units() + 63).div_euclid(64)
}

fn pixels(length: i32) -> Checked<u32> {
    u32::try_from(length).map_err(failed)
}

/// The encoding of the drawing of `shaped` on its canvas with a margin of
/// `margin` pixels: the glyph masks of its list prepared in `set` through
/// `rasterizer`, which then draws the list from them.
pub fn draw(
    set: &mut FontSet,
    rasterizer: &mut dyn Rasterizer,
    shaped: &Shaped,
    margin: i32,
) -> Checked<Vec<u8>> {
    let (list, desc) = scene(set, shaped, margin)?;
    set.prepare(&list, rasterizer).map_err(failed)?;
    let mut target = RenderTarget::new(desc).ok_or("no canvas")?;
    let whole = target.whole();
    rasterizer
        .render(&list, &whole, set.glyphs(), &ImageStore::EMPTY, &mut target)
        .map_err(failed)?;
    Ok(encode(target.desc(), target.bytes()))
}

/// The encoding of an `Rgba8Premul` buffer of `desc` that holds `pixels`.
pub fn encode(desc: BufferDesc, pixels: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEADER + pixels.len());
    for field in [desc.width(), desc.height(), desc.stride(), RGBA8_PREMUL] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(pixels);
    bytes
}

/// The width, height, stride and format code of an encoding.
pub fn description(bytes: &[u8]) -> Option<[u32; 4]> {
    let (words, _) = bytes.get(..HEADER)?.as_chunks::<4>();
    let field = |at: usize| words.get(at).map(|word| u32::from_le_bytes(*word));
    Some([field(0)?, field(1)?, field(2)?, field(3)?])
}

/// Compares the encoding `drawn` with `golden`, the golden `name`, byte
/// for byte. A difference fails with both descriptions when they differ,
/// and otherwise with the first pixel that differs, its position and its
/// bytes in both.
pub fn compare(name: &str, golden: &[u8], drawn: &[u8]) -> Checked {
    if golden == drawn {
        return Ok(());
    }
    let (shown, expected) = (description(drawn), description(golden));
    let Some([_, _, stride, _]) = shown.filter(|_| shown == expected) else {
        return Err(format!(
            "{name}: drawn {shown:?}, but the golden is {expected:?}, as width, height, stride and format code"
        ));
    };
    let (golden, drawn) = (&golden[HEADER..], &drawn[HEADER..]);
    let Some(at) = golden.iter().zip(drawn).position(|(a, b)| a != b) else {
        let (drawn, golden) = (drawn.len(), golden.len());
        return Err(format!(
            "{name}: {drawn} bytes drawn, but the golden has {golden}"
        ));
    };
    let stride = (stride as usize).max(1);
    let (x, y) = ((at % stride) / 4, at / stride);
    let start = y * stride + x * 4;
    let pixel = |bytes: &[u8]| match bytes.get(start..start + 4) {
        Some(pixel) => format!("{pixel:02x?}"),
        None => "cut off".to_string(),
    };
    let (new, old) = (pixel(drawn), pixel(golden));
    Err(format!(
        "{name}: pixel ({x}, {y}) is {new} drawn and {old} in the golden"
    ))
}

/// The encodings of the strings, by name, drawn in their order through one
/// font set and one vello_cpu rasterizer, so that each finds the glyph
/// cache as the strings before it left it.
pub fn drawings() -> Checked<Vec<(&'static str, Vec<u8>)>> {
    let mut set = font_set()?;
    let mut rasterizer = VelloRasterizer::new();
    let mut drawings = Vec::new();
    for shaped in &STRINGS {
        let drawn = draw(&mut set, &mut rasterizer, shaped, MARGIN)?;
        drawings.push((shaped.name, drawn));
    }
    Ok(drawings)
}

/// The drawing of the string `name` matches the golden `name` byte for
/// byte. The goldens step of `cargo xtask check` finds the goldens that
/// the tests refer to by the calls of this function with a string literal.
pub fn matches_golden(name: &str) -> Checked {
    matches_file_in(GOLDENS, name)
}

/// The string `name`, drawn alone through a new font set and a new
/// vello_cpu rasterizer, matches the file `<dir>/<name>.px` byte for byte.
pub fn matches_file_in(dir: &str, name: &str) -> Checked {
    let shaped = STRINGS
        .iter()
        .find(|shaped| shaped.name == name)
        .ok_or_else(|| format!("no string is named {name}"))?;
    let drawn = draw(
        &mut font_set()?,
        &mut VelloRasterizer::new(),
        shaped,
        MARGIN,
    )?;
    let path = format!("{dir}/{name}.px");
    let golden = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    compare(name, &golden, &drawn)
}

/// Writes the golden of each string into `dir`.
pub fn write(dir: &str) -> Checked {
    fs::create_dir_all(dir).map_err(failed)?;
    for (name, bytes) in drawings()? {
        fs::write(format!("{dir}/{name}.px"), bytes).map_err(failed)?;
    }
    Ok(())
}
