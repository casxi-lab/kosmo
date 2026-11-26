//! Support of the integration tests: the per-pixel reference rasterizer, a
//! glyph coverage of three test glyphs and the scenes that the tests draw.
//! Each test file declares it `pub`, so the parts that it does not use are
//! not reported as unused.

pub mod glyphs;
pub mod reference;
pub mod scenes;

/// The result of a helper; the error says what went wrong.
pub type Checked<T = ()> = Result<T, String>;

/// `error` as the error of a helper.
pub fn failed(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
