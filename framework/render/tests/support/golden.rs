//! Pixel goldens: the encoding of a drawn buffer, the comparison of a
//! drawing with its golden and the writer of the goldens.
//!
//! The golden `<name>` is the file `tests/golden/<name>.px`: the
//! description of the buffer in 16 bytes, its width, height, stride and
//! format code as little-endian `u32`s, followed by the bytes of the
//! buffer. The format codes are 1 for `Rgba8Premul`, 2 for `Xrgb8888`, 3
//! for `Rgb565` and 4 for `Argb8888Premul`. `cargo xtask golden` runs the
//! writers and lists the digests of the files in `tests/golden/MANIFEST`;
//! the other tests only read the goldens.

use std::fs;

use kosmo_render::{RenderTarget, VelloRasterizer};
use kosmo_types::PixelFormat;

use super::scenes;
use super::{Checked, failed};

/// The directory of the goldens.
pub const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");

/// The directory, in the workspace's build directory, into which
/// `cargo xtask golden --verify` draws the goldens again.
pub const REDRAWN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/golden/kosmo-render");

/// The bytes of the encoded description.
const HEADER: usize = 16;

/// The formats with their codes in the encoding.
const FORMAT_CODES: [(PixelFormat, u32); 4] = [
    (PixelFormat::Rgba8Premul, 1),
    (PixelFormat::Xrgb8888, 2),
    (PixelFormat::Rgb565, 3),
    (PixelFormat::Argb8888Premul, 4),
];

/// The format code of `format` in the encoding.
pub fn format_code(format: PixelFormat) -> Checked<u32> {
    FORMAT_CODES
        .iter()
        .find(|&&(known, _)| known == format)
        .map(|&(_, code)| code)
        .ok_or_else(|| format!("{format:?} has no format code"))
}

/// The format of the format code `code` in the encoding.
fn format_of(code: u32) -> Checked<PixelFormat> {
    FORMAT_CODES
        .iter()
        .find(|&&(_, known)| known == code)
        .map(|&(format, _)| format)
        .ok_or_else(|| format!("format code {code} is unknown"))
}

/// The encoding of `target`: its description and its bytes.
pub fn encode(target: &RenderTarget) -> Checked<Vec<u8>> {
    let desc = target.desc();
    let code = format_code(desc.format())?;
    let mut bytes = Vec::with_capacity(HEADER + target.bytes().len());
    for field in [desc.width(), desc.height(), desc.stride(), code] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.extend_from_slice(target.bytes());
    Ok(bytes)
}

/// The width, height, stride and format code of an encoding.
fn description(bytes: &[u8]) -> Option<[u32; 4]> {
    let (words, _) = bytes.get(..HEADER)?.as_chunks::<4>();
    let field = |at: usize| words.get(at).map(|word| u32::from_le_bytes(*word));
    Some([field(0)?, field(1)?, field(2)?, field(3)?])
}

/// The description of an encoding as an error shows it.
fn shown(bytes: &[u8]) -> String {
    match description(bytes) {
        Some([width, height, stride, code]) => {
            format!("{width}x{height}, stride {stride}, format {code}")
        }
        None => format!("{} bytes, shorter than a description", bytes.len()),
    }
}

/// Compares the encoding `drawn` with `golden`, the golden `name`, byte
/// for byte. A difference fails with the first pixel that differs, its
/// position and its bytes in both, or with the format code when it is
/// unknown.
pub fn compare(name: &str, golden: &[u8], drawn: &[u8]) -> Checked {
    if golden == drawn {
        return Ok(());
    }
    let Some([width, _, stride, code]) =
        description(drawn).filter(|_| shown(golden) == shown(drawn))
    else {
        let (drawn, golden) = (shown(drawn), shown(golden));
        return Err(format!("{name}: drawn {drawn}, but the golden is {golden}"));
    };
    let lengths = (drawn.len(), golden.len());
    let (golden, drawn) = (&golden[HEADER..], &drawn[HEADER..]);
    let Some(at) = golden.iter().zip(drawn).position(|(a, b)| a != b) else {
        let (drawn, golden) = lengths;
        return Err(format!(
            "{name}: {drawn} bytes drawn, but the golden has {golden}"
        ));
    };
    let bpp = format_of(code)
        .map_err(|error| format!("{name}: {error}"))?
        .bytes_per_pixel() as usize;
    let stride = (stride as usize).max(1);
    let (y, column) = (at / stride, at % stride);
    if column >= width as usize * bpp {
        let (new, old) = (drawn[at], golden[at]);
        return Err(format!(
            "{name}: byte {column} of row {y}, after its pixels, is {new:#04x} drawn and {old:#04x} in the golden"
        ));
    }
    let x = column / bpp;
    let start = y * stride + x * bpp;
    let pixel = |bytes: &[u8]| match bytes.get(start..start + bpp) {
        Some(pixel) => format!("{pixel:02x?}"),
        None => "cut off".to_string(),
    };
    let (new, old) = (pixel(drawn), pixel(golden));
    Err(format!(
        "{name}: pixel ({x}, {y}) is {new} drawn and {old} in the golden"
    ))
}

/// The encodings of the scenes of one kind of command, by name, drawn by
/// one new vello_cpu rasterizer, which also fills the glyph masks.
pub fn drawings() -> Checked<Vec<(&'static str, Vec<u8>)>> {
    let mut rasterizer = VelloRasterizer::new();
    let scenes = scenes::primitives(&mut rasterizer)?;
    let mut drawings = Vec::new();
    for scene in scenes {
        drawings.push((scene.name, encode(&scene.draw(&mut rasterizer)?)?));
    }
    Ok(drawings)
}

/// The drawing of the scene `name` matches the golden `name` byte for
/// byte. The goldens step of `cargo xtask check` finds the goldens that
/// the tests refer to by the calls of this function with a string literal.
pub fn matches_golden(name: &str) -> Checked {
    matches_file_in(GOLDENS, name)
}

/// The scene `name`, drawn alone by one new vello_cpu rasterizer, which
/// also fills the glyph masks, matches the file `<dir>/<name>.px` byte for
/// byte.
pub fn matches_file_in(dir: &str, name: &str) -> Checked {
    let mut rasterizer = VelloRasterizer::new();
    let scene = scenes::primitives(&mut rasterizer)?
        .into_iter()
        .find(|scene| scene.name == name)
        .ok_or_else(|| format!("no scene is named {name}"))?;
    let drawn = encode(&scene.draw(&mut rasterizer)?)?;
    let path = format!("{dir}/{name}.px");
    let golden = fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    compare(name, &golden, &drawn)
}

/// Writes the golden of each scene of one kind of command into `dir`.
pub fn write(dir: &str) -> Checked {
    fs::create_dir_all(dir).map_err(failed)?;
    for (name, bytes) in drawings()? {
        fs::write(format!("{dir}/{name}.px"), bytes).map_err(failed)?;
    }
    Ok(())
}
