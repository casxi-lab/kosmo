//! Support of the integration tests: the per-pixel reference rasterizer, a
//! glyph coverage of three test glyphs, the scenes that the tests draw and
//! their pixel goldens.
//! Each test file declares it `pub`, so the parts that it does not use are
//! not reported as unused.

pub mod glyphs;
pub mod golden;
pub mod reference;
pub mod scenes;

/// The result of a helper; the error says what went wrong.
pub type Checked<T = ()> = Result<T, String>;

/// `error` as the error of a helper.
pub fn failed(error: impl std::fmt::Debug) -> String {
    format!("{error:?}")
}
