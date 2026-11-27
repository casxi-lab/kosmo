//! Pixel digests: the SHA-256 of a buffer's description and bytes, the
//! encoding of the pixel goldens of `kosmo-render`, so the digest of a
//! presented frame equals the digest that a golden's `MANIFEST` lists for
//! the same frame.
//!
//! The description takes 16 bytes: the width, the height, the stride and a
//! format code, each a little-endian `u32`, with the codes 1 for
//! `Rgba8Premul`, 2 for `Xrgb8888`, 3 for `Rgb565` and 4 for
//! `Argb8888Premul`. The bytes of the buffer follow as they are, without
//! image encoding. The digest is computed with the `sha2` package, pinned
//! exactly, not by hand.
//!
//! ```
//! use kosmo_testkit::pixels::PixelDigest;
//! use kosmo_types::{BufferDesc, PixelFormat};
//!
//! let desc = BufferDesc::new(2, 1, 8, PixelFormat::Xrgb8888).unwrap();
//! let digest = PixelDigest::of(desc, &[0, 0, 255, 255, 0, 0, 255, 255]).unwrap();
//! assert!(digest.to_string().starts_with("2x1 stride=8 xrgb8888 sha256="));
//! assert_eq!(digest.to_string().len(), "2x1 stride=8 xrgb8888 sha256=".len() + 64);
//! ```

use std::fmt;

use kosmo_types::{BufferDesc, PixelFormat};
use sha2::{Digest, Sha256};

/// The pixel formats with their codes in the encoding and their names in
/// scripts and records.
const FORMATS: [(PixelFormat, u32, &str); 4] = [
    (PixelFormat::Rgba8Premul, 1, "rgba8-premul"),
    (PixelFormat::Xrgb8888, 2, "xrgb8888"),
    (PixelFormat::Rgb565, 3, "rgb565"),
    (PixelFormat::Argb8888Premul, 4, "argb8888-premul"),
];

/// The code of `format` in the encoding, or `None` for a format that this
/// version does not know.
pub fn format_code(format: PixelFormat) -> Option<u32> {
    FORMATS
        .iter()
        .find(|&&(known, _, _)| known == format)
        .map(|&(_, code, _)| code)
}

/// The name of `format` in scripts and records: `rgba8-premul`,
/// `xrgb8888`, `rgb565` or `argb8888-premul`.
pub fn format_name(format: PixelFormat) -> Option<&'static str> {
    FORMATS
        .iter()
        .find(|&&(known, _, _)| known == format)
        .map(|&(_, _, name)| name)
}

/// The format named `name` in scripts and records.
pub fn format_named(name: &str) -> Option<PixelFormat> {
    FORMATS
        .iter()
        .find(|&&(_, _, known)| known == name)
        .map(|&(format, _, _)| format)
}

/// The digest of a buffer: its description and the SHA-256 of the
/// description and the bytes, as the module documentation states.
///
/// Its text, as `Display` writes it, is
/// `<width>x<height> stride=<stride> <format> sha256=<digest>`, with the
/// format's name and the digest in 64 lowercase hexadecimal digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelDigest {
    desc: BufferDesc,
    sha256: [u8; 32],
}

impl PixelDigest {
    /// The digest of a buffer of `desc` that holds `bytes`, or `None` when
    /// this version knows no code for the format of `desc`.
    pub fn of(desc: BufferDesc, bytes: &[u8]) -> Option<PixelDigest> {
        let code = format_code(desc.format())?;
        let mut hasher = Sha256::new();
        for field in [desc.width(), desc.height(), desc.stride(), code] {
            hasher.update(field.to_le_bytes());
        }
        hasher.update(bytes);
        Some(PixelDigest {
            desc,
            sha256: hasher.finalize().into(),
        })
    }

    /// The description of the buffer.
    pub fn desc(&self) -> BufferDesc {
        self.desc
    }

    /// The SHA-256 of the description and the bytes.
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    /// The SHA-256 in 64 lowercase hexadecimal digits, as `MANIFEST`
    /// writes the digest of a golden.
    pub fn hex(&self) -> String {
        self.sha256
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

impl fmt::Display for PixelDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} stride={} {} sha256={}",
            self.desc.width(),
            self.desc.height(),
            self.desc.stride(),
            format_name(self.desc.format()).unwrap_or("unknown"),
            self.hex()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A golden of `kosmo-render`, in the encoding of the goldens.
    const GOLDEN: &[u8] = include_bytes!("../../render/tests/golden/fill-rect.px");

    /// The digests of the goldens.
    const MANIFEST: &str = include_str!("../../render/tests/golden/MANIFEST");

    #[test]
    fn the_digest_of_a_golden_frame_is_its_manifest_digest() {
        let (header, bytes) = GOLDEN.split_at(16);
        let word = |at: usize| u32::from_le_bytes(header[at * 4..at * 4 + 4].try_into().unwrap());
        let format = FORMATS
            .iter()
            .find(|&&(_, code, _)| code == word(3))
            .map(|&(format, _, _)| format)
            .unwrap();
        let desc = BufferDesc::new(word(0), word(1), word(2), format).unwrap();
        let digest = PixelDigest::of(desc, bytes).unwrap();
        let listed = MANIFEST
            .lines()
            .find_map(|line| line.strip_suffix("  fill-rect"))
            .unwrap();
        assert_eq!(digest.hex(), listed);
        assert_eq!(digest.desc(), desc);
    }

    #[test]
    fn the_description_is_part_of_the_digest() {
        let bytes = [0x80; 16];
        let wide = BufferDesc::new(4, 1, 16, PixelFormat::Rgba8Premul).unwrap();
        let tall = BufferDesc::new(1, 4, 4, PixelFormat::Rgba8Premul).unwrap();
        let other = BufferDesc::new(4, 1, 16, PixelFormat::Xrgb8888).unwrap();
        let digests = [wide, tall, other].map(|desc| PixelDigest::of(desc, &bytes).unwrap());
        assert_ne!(digests[0].sha256(), digests[1].sha256());
        assert_ne!(digests[0].sha256(), digests[2].sha256());
        assert_eq!(
            digests[0].to_string(),
            format!("4x1 stride=16 rgba8-premul sha256={}", digests[0].hex())
        );
    }

    #[test]
    fn every_format_has_a_code_and_a_name() {
        for (format, code, name) in FORMATS {
            assert_eq!(format_code(format), Some(code));
            assert_eq!(format_name(format), Some(name));
            assert_eq!(format_named(name), Some(format));
        }
        assert_eq!(format_named("rgba8"), None);
    }
}
