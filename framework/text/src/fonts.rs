//! The font set: the two bundled fonts, registered from their bytes for the
//! text stack, without system fonts.

use core::fmt;

use kosmo_render::FontId;
use parley::FontContext;
use parley::fontique::{Blob, Collection, CollectionOptions, FamilyId, SourceCache};
use skrifa::{FontRef, Tag};

/// Fira Sans, the proportional interface font.
pub const SANS: FontId = FontId::new(0);

/// Fira Mono, the monospaced font.
pub const MONO: FontId = FontId::new(1);

/// The file of Fira Sans in the asset manifest, relative to the asset
/// directory, which names the font as a resource of the platform
/// interface.
pub const SANS_FILE: &str = "firasans/FiraSans-Regular.ttf";

/// The file of Fira Mono in the asset manifest, relative to the asset
/// directory, which names the font as a resource of the platform
/// interface.
pub const MONO_FILE: &str = "firamono/FiraMono-Regular.ttf";

/// Why [`FontSet::new`] cannot build a font set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSetError {
    /// The bytes given for the font are not one TrueType font: they are not
    /// a font file, hold a collection of fonts, have no TrueType outlines or
    /// are not registered as one font by fontique, which skips a font without
    /// a `name` table.
    NotTrueType(FontId),
    /// Both fonts belong to the same family.
    SameFamily,
}

impl fmt::Display for FontSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontSetError::NotTrueType(font) => {
                write!(
                    f,
                    "the bytes of font {} are not one TrueType font",
                    font.get()
                )
            }
            FontSetError::SameFamily => f.write_str("both fonts belong to the same family"),
        }
    }
}

impl core::error::Error for FontSetError {}

/// The two fonts of the framework, [`SANS`] and [`MONO`], registered from
/// their bytes.
///
/// The set holds a copy of the bytes and reads no files. Its collection of
/// fonts has system fonts off, no generic families and no fallback, so text
/// that it lays out uses only these two fonts.
pub struct FontSet {
    context: FontContext,
    sans: FamilyId,
    mono: FamilyId,
}

impl FontSet {
    /// The font set of Fira Sans with the bytes `sans` and Fira Mono with
    /// the bytes `mono`, the files [`SANS_FILE`] and [`MONO_FILE`] of the
    /// asset directory.
    pub fn new(sans: &[u8], mono: &[u8]) -> Result<FontSet, FontSetError> {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: false,
        });
        let sans = register(&mut collection, SANS, sans)?;
        let mono = register(&mut collection, MONO, mono)?;
        if sans == mono {
            return Err(FontSetError::SameFamily);
        }
        Ok(FontSet {
            context: FontContext {
                collection,
                source_cache: SourceCache::default(),
            },
            sans,
            mono,
        })
    }

    /// The family name of `font`, as its font file names it, or `None` for
    /// a font other than [`SANS`] and [`MONO`].
    pub fn family_name(&mut self, font: FontId) -> Option<&str> {
        let family = match font {
            SANS => self.sans,
            MONO => self.mono,
            _ => return None,
        };
        self.context.collection.family_name(family)
    }
}

/// Registers the bytes of `font` in `collection` and returns its family.
fn register(
    collection: &mut Collection,
    font: FontId,
    bytes: &[u8],
) -> Result<FamilyId, FontSetError> {
    let not_true_type = FontSetError::NotTrueType(font);
    let parsed = FontRef::new(bytes).map_err(|_| not_true_type)?;
    let outlines = [b"glyf", b"loca"]
        .into_iter()
        .all(|tag| parsed.table_data(Tag::new(tag)).is_some());
    if !outlines {
        return Err(not_true_type);
    }
    match collection
        .register_fonts(Blob::from(bytes.to_vec()), None)
        .as_slice()
    {
        [(family, fonts)] if fonts.len() == 1 => Ok(*family),
        _ => Err(not_true_type),
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    const SANS_BYTES: &[u8] = include_bytes!("../assets/fonts/firasans/FiraSans-Regular.ttf");
    const MONO_BYTES: &[u8] = include_bytes!("../assets/fonts/firamono/FiraMono-Regular.ttf");

    /// Fira Mono with the last letter of the tag of its table `tag` changed,
    /// so that it has no table `tag`.
    fn mono_without_table(tag: &[u8; 4]) -> Vec<u8> {
        let mut renamed = MONO_BYTES.to_vec();
        let tables = usize::from(u16::from_be_bytes([renamed[4], renamed[5]]));
        let record = (0..tables)
            .map(|index| 12 + 16 * index)
            .find(|&at| &renamed[at..at + 4] == tag)
            .unwrap();
        renamed[record + 3] = b'x';
        renamed
    }

    #[test]
    fn the_font_set_has_only_the_two_bundled_families() {
        assert_eq!((SANS.get(), MONO.get()), (0, 1));
        let mut set = FontSet::new(SANS_BYTES, MONO_BYTES).unwrap();
        let mut families: Vec<&str> = set.context.collection.family_names().collect();
        families.sort_unstable();
        assert_eq!(families, ["Fira Mono", "Fira Sans"]);
        assert_eq!(set.family_name(SANS), Some("Fira Sans"));
        assert_eq!(set.family_name(MONO), Some("Fira Mono"));
        assert_eq!(set.family_name(FontId::new(2)), None);
        let collection = &mut set.context.collection;
        assert_eq!(collection.family_ids().count(), 2);
        for family in [set.sans, set.mono] {
            let fonts = collection.family(family).unwrap().fonts().len();
            assert_eq!(fonts, 1);
        }
    }

    #[test]
    fn bytes_that_are_not_one_true_type_font_name_their_font() {
        let license = include_bytes!("../assets/fonts/firamono/OFL.txt");
        assert_eq!(
            FontSet::new(b"", MONO_BYTES).err(),
            Some(FontSetError::NotTrueType(SANS))
        );
        assert_eq!(
            FontSet::new(SANS_BYTES, license).err(),
            Some(FontSetError::NotTrueType(MONO))
        );
        // Fira Mono with its `glyf` table renamed parses as a font without
        // TrueType outlines.
        let renamed = mono_without_table(b"glyf");
        assert!(FontRef::new(&renamed).is_ok());
        assert_eq!(
            FontSet::new(SANS_BYTES, &renamed).err(),
            Some(FontSetError::NotTrueType(MONO))
        );
    }

    #[test]
    fn a_font_without_a_name_table_is_not_registered() {
        // Fira Mono with its `name` table renamed parses as a font with
        // TrueType outlines, but fontique skips a font without a `name`
        // table and registers nothing.
        let renamed = mono_without_table(b"name");
        let parsed = FontRef::new(&renamed).unwrap();
        for tag in [b"glyf", b"loca"] {
            assert!(parsed.table_data(Tag::new(tag)).is_some());
        }
        assert_eq!(
            FontSet::new(SANS_BYTES, &renamed).err(),
            Some(FontSetError::NotTrueType(MONO))
        );
    }

    #[test]
    fn the_same_family_twice_is_rejected() {
        assert_eq!(
            FontSet::new(SANS_BYTES, SANS_BYTES).err(),
            Some(FontSetError::SameFamily)
        );
    }

    #[test]
    fn the_file_names_are_those_of_the_asset_manifest() {
        let manifest = include_str!("../assets/fonts/MANIFEST.toml");
        for file in [SANS_FILE, MONO_FILE] {
            let line = alloc::format!("file = \"{file}\"\n");
            assert_eq!(manifest.matches(&line).count(), 1, "{file}");
        }
    }
}
