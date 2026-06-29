//! FNV-1a — the SINGLE source of truth for the non-cryptographic hashes used
//! across the harness. These hashes are *load-bearing for on-disk addressing*:
//! `projkey::project_key` derives a project's state-dir name from `fnv1a32`, so
//! if one crate's private copy of the algorithm drifted by a single constant,
//! that crate would silently read/write a *different* directory and lose state.
//! Folding every copy onto this module makes such drift impossible — there is
//! one implementation, pinned by the regression vectors below.
//!
//! Two widths are provided because the call sites genuinely need both:
//!   * 32-bit (`fnv1a32`) — project keys, session tags, backlog task ids.
//!   * 64-bit (`fnv1a64` / `Fnv1a64`) — specguard prompt/spec fingerprints, and
//!     hypothesis ids (which take the low 32 bits of the 64-bit hash).
//!
//! FNV-1a is order-dependent and fully streaming: feeding bytes incrementally
//! through `Fnv1a64::update` yields the identical result to a one-shot
//! `fnv1a64` over the same concatenated byte stream. `fingerprint`-style sites
//! that accumulate many fields therefore keep their exact values.

/// FNV-1a 32-bit offset basis.
const OFFSET32: u32 = 0x811c_9dc5;
/// FNV-1a 32-bit prime.
const PRIME32: u32 = 0x0100_0193;
/// FNV-1a 64-bit offset basis.
const OFFSET64: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const PRIME64: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 32-bit hash of a string's UTF-8 bytes. Small, dependency-free, stable
/// across runs and platforms.
pub fn fnv1a32(s: &str) -> u32 {
    let mut h = OFFSET32;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(PRIME32);
    }
    h
}

/// FNV-1a 32-bit hash, formatted as exactly 8 lowercase hex digits.
pub fn fnv1a32_hex(s: &str) -> String {
    format!("{:08x}", fnv1a32(s))
}

/// One-shot FNV-1a 64-bit hash over a byte slice.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = Fnv1a64::new();
    h.update(bytes);
    h.finish()
}

/// Incremental FNV-1a 64-bit hasher, for fingerprints accumulated over many
/// fields. `update`-ing chunks is byte-for-byte equivalent to one `fnv1a64`
/// call over their concatenation, so streaming sites keep their exact hash.
#[derive(Clone, Copy, Debug)]
pub struct Fnv1a64 {
    state: u64,
}

impl Fnv1a64 {
    /// A fresh hasher seeded with the 64-bit offset basis.
    pub fn new() -> Self {
        Self { state: OFFSET64 }
    }

    /// Fold a chunk of bytes into the running hash.
    pub fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= b as u64;
            self.state = self.state.wrapping_mul(PRIME64);
        }
    }

    /// The current 64-bit hash value.
    pub fn finish(&self) -> u64 {
        self.state
    }
}

impl Default for Fnv1a64 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a32_known_vectors() {
        // Empty string hashes to the offset basis (defining property of FNV-1a).
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        // A second vector pins the multiply/xor step so the algorithm can't
        // silently change — that would relocate every project's state dir.
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
        // Canonical reference vector for "foobar".
        assert_eq!(fnv1a32("foobar"), 0xbf9c_f968);
    }

    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn hex_helper_is_zero_padded_to_eight() {
        assert_eq!(fnv1a32_hex(""), "811c9dc5");
        assert_eq!(fnv1a32_hex("a"), "e40c292c");
    }

    #[test]
    fn incremental_equals_one_shot() {
        // The streaming property the `fingerprint` sites rely on: updating in
        // chunks must equal hashing the concatenation in one go.
        let mut h = Fnv1a64::new();
        h.update(b"foo");
        h.update(b"bar");
        assert_eq!(h.finish(), fnv1a64(b"foobar"));

        let mut empty = Fnv1a64::new();
        empty.update(b"");
        assert_eq!(empty.finish(), fnv1a64(b""));
    }

    #[test]
    fn hypothesis_id_low32_pin() {
        // hypothesis::new_id takes the low 32 bits of the 64-bit hash as 8 hex.
        // Pin that exact derivation so the id scheme can't drift.
        assert_eq!(format!("{:08x}", fnv1a64(b"foobar") as u32), "f73967e8");
    }
}
