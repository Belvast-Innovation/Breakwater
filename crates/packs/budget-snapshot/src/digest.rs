//! A small, stable digest of the generated workload.
//!
//! FNV-1a 64 is not cryptographic. It only has to notice that the sequence a
//! suite generated is not the one a snapshot was taken with, and, unlike
//! `std::hash::DefaultHasher`, its output is specified and never changes
//! between Rust releases.

const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Incremental FNV-1a 64.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fnv1a64(u64);

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self(OFFSET_BASIS)
    }
}

impl Fnv1a64 {
    /// Feeds `bytes` into the digest.
    pub fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(PRIME);
        }
    }

    /// Feeds one field and a separator that cannot occur in UTF-8 text, so
    /// `("ab", "c")` and `("a", "bc")` digest differently.
    pub fn field(&mut self, bytes: &[u8]) {
        self.update(bytes);
        self.update(&[0xff]);
    }

    /// The raw 64-bit value.
    pub fn value(&self) -> u64 {
        self.0
    }

    /// The value as it is stored in a snapshot: `fnv1a64:<16 hex digits>`.
    pub fn to_tagged_hex(&self) -> String {
        format!("fnv1a64:{:016x}", self.0)
    }
}

/// The FNV-1a 64 value of `bytes`.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut digest = Fnv1a64::default();
    digest.update(bytes);
    digest.value()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn fields_are_delimited() {
        let mut a = Fnv1a64::default();
        a.field(b"ab");
        a.field(b"c");
        let mut b = Fnv1a64::default();
        b.field(b"a");
        b.field(b"bc");
        assert_ne!(a, b);
    }

    #[test]
    fn order_matters_and_the_tag_is_fixed_width() {
        let mut a = Fnv1a64::default();
        a.field(b"x");
        a.field(b"y");
        let mut b = Fnv1a64::default();
        b.field(b"y");
        b.field(b"x");
        assert_ne!(a, b);
        assert_eq!(a.to_tagged_hex().len(), "fnv1a64:".len() + 16);
        assert!(Fnv1a64::default()
            .to_tagged_hex()
            .starts_with("fnv1a64:cbf29ce484222325"));
    }
}
