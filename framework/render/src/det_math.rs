//! The transcendental functions of deterministic code: code that writes
//! layout coordinates, layout trees, display lists, glyph positions,
//! coverage or pixels calls them only through this module.
//!
//! Each function takes and returns only `f32` or `f64` values and forwards
//! to `libm` 0.2.16, pinned exactly, without an approximation of its own,
//! so it gives the same result on every host. The methods of `f32` and
//! `f64` of the same names may call the host's math library, whose results
//! differ between hosts. A function is added here with its first caller,
//! together with a unit test.

/// The sine of `x`, in radians, from `libm`'s `sin`.
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

/// The sine of `x`, in radians, from `libm`'s `sinf`.
pub fn sinf(x: f32) -> f32 {
    libm::sinf(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_of_one_is_libm_sin() {
        // 0.8414709848078965, the f64 nearest to the sine of 1.
        assert_eq!(sin(1.0).to_bits(), 0x3FEA_ED54_8F09_0CEE);
    }

    #[test]
    fn sinf_of_one_is_libm_sinf() {
        // 0.84147096, the f32 nearest to the sine of 1.
        assert_eq!(sinf(1.0).to_bits(), 0x3F57_6AA4);
    }
}
