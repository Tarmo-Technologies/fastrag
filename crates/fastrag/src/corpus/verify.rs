//! MinHash-based near-duplicate verifier for POST /similar.
//!
//! Deterministic, seedable, no I/O. The public surface is:
//!
//! * [`signature_of`] — produce a [`Signature`] for a string.
//! * [`jaccard`] — estimate Jaccard similarity over two signatures.
//! * [`VerifyConfig`] / [`VerifyMethod`] — request types carried on
//!   `SimilarityRequest`.
//!
//! Construction:
//!
//! * Char-5gram shingling, lowercase, ASCII punctuation stripped, whitespace
//!   collapsed to single spaces. Inputs shorter than 5 chars emit an empty
//!   shingle set and produce the all-zero signature.
//! * blake3 of each shingle, truncated to the first 8 bytes as a `u64`, serves
//!   as the base hash.
//! * 128 independent permutations via `h_i(x) = (a_i * x + b_i) mod p` with
//!   `p = 2^61 - 1`. Coefficients are generated once (SplitMix64 from a fixed
//!   `PERMUTATION_SEED`) and memoized via `OnceLock`.
//! * Signature is `[u64; 128]`; Jaccard is `count(equal lanes) / 128`.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Number of MinHash permutations. Changing this invalidates the pinned
/// fixture signature in the test module.
pub const NUM_PERMUTATIONS: usize = 128;

/// Char-n-gram shingle size in bytes.
pub const SHINGLE_SIZE: usize = 5;

/// 2^61 - 1 — a Mersenne prime large enough for u64 linear hashing.
const MERSENNE_61: u64 = (1u64 << 61) - 1;

/// Seed for deterministic `(a_i, b_i)` permutation coefficients. Do NOT change
/// without updating the fixture regression test.
const PERMUTATION_SEED: u64 = 0xFA57_5A60_DEDB_5EED;

/// One MinHash signature. Fixed size to avoid per-shingle allocation.
pub type Signature = [u64; NUM_PERMUTATIONS];

/// Runtime config carried on a `SimilarityRequest`.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyConfig {
    pub method: VerifyMethod,
    pub threshold: f32,
}

/// Which verifier to run. Only `MinHash` is accepted in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyMethod {
    MinHash,
}

/// Compute a MinHash signature over char-5gram shingles of `text`.
pub fn signature_of(text: &str) -> Signature {
    let normalized = normalize(text);
    let bytes = normalized.as_bytes();
    if bytes.len() < SHINGLE_SIZE {
        return [0u64; NUM_PERMUTATIONS];
    }
    let (a, b) = permutation_coeffs();
    let mut sig = [u64::MAX; NUM_PERMUTATIONS];
    for start in 0..=bytes.len() - SHINGLE_SIZE {
        let end = start + SHINGLE_SIZE;
        let shingle = &bytes[start..end];
        let base = base_hash_u64(shingle);
        for i in 0..NUM_PERMUTATIONS {
            let prod = mul_mod_mersenne(a[i], base);
            let h = add_mod_mersenne(prod, b[i]);
            if h < sig[i] {
                sig[i] = h;
            }
        }
    }
    sig
}

/// Estimated Jaccard similarity over two signatures. Range `[0.0, 1.0]`.
pub fn jaccard(a: &Signature, b: &Signature) -> f32 {
    let mut equal = 0usize;
    for i in 0..NUM_PERMUTATIONS {
        if a[i] == b[i] {
            equal += 1;
        }
    }
    equal as f32 / NUM_PERMUTATIONS as f32
}

/// Normalize: lowercase, ASCII punctuation stripped (treated as whitespace),
/// whitespace collapsed to single ASCII spaces, leading/trailing trimmed.
fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = true; // avoids leading whitespace
    for c in text.chars() {
        if c.is_ascii_punctuation() || c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
            continue;
        }
        for lc in c.to_lowercase() {
            out.push(lc);
        }
        prev_ws = false;
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn base_hash_u64(shingle: &[u8]) -> u64 {
    let digest = blake3::hash(shingle);
    let bytes = digest.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]) % MERSENNE_61
}

fn permutation_coeffs() -> &'static ([u64; NUM_PERMUTATIONS], [u64; NUM_PERMUTATIONS]) {
    static COEFFS: OnceLock<([u64; NUM_PERMUTATIONS], [u64; NUM_PERMUTATIONS])> = OnceLock::new();
    COEFFS.get_or_init(|| {
        let mut state = PERMUTATION_SEED;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let mut a = [0u64; NUM_PERMUTATIONS];
        let mut b = [0u64; NUM_PERMUTATIONS];
        for i in 0..NUM_PERMUTATIONS {
            let mut ai = next() % MERSENNE_61;
            if ai == 0 {
                ai = 1;
            }
            a[i] = ai;
            b[i] = next() % MERSENNE_61;
        }
        (a, b)
    })
}

/// Reduce `x` mod `2^61 - 1`. Works for any `u64` input.
fn mod_mersenne(x: u64) -> u64 {
    let lo = x & MERSENNE_61;
    let hi = x >> 61;
    let r = lo + hi;
    if r >= MERSENNE_61 { r - MERSENNE_61 } else { r }
}

/// `(a + b) mod (2^61 - 1)` where `a, b < 2^61 - 1`.
fn add_mod_mersenne(a: u64, b: u64) -> u64 {
    let s = a.wrapping_add(b);
    // a + b fits in u64 because both inputs are < 2^61.
    mod_mersenne(s)
}

/// `(a * b) mod (2^61 - 1)` for arbitrary `u64` inputs. Uses `u128` to hold
/// the full product, then folds the high bits back in via the
/// `2^61 ≡ 1 (mod 2^61 - 1)` identity.
fn mul_mod_mersenne(a: u64, b: u64) -> u64 {
    let prod = (a as u128) * (b as u128);
    // Split into hi (bits 61..) and lo (bits 0..61). 2^61 ≡ 1 (mod p), so
    // the value is hi + lo (mod p).
    let lo = (prod & (MERSENNE_61 as u128)) as u64;
    let hi = (prod >> 61) as u64;
    // hi can be up to ~2^67, so fold it again.
    add_mod_mersenne(mod_mersenne(hi), lo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_strips_punctuation_collapses_whitespace() {
        let got = normalize("Hello, World!  \tHi.");
        assert_eq!(got, "hello world hi");
    }

    #[test]
    fn normalize_trims_leading_and_trailing() {
        assert_eq!(normalize("   abc   "), "abc");
        assert_eq!(normalize("!!hi!!"), "hi");
    }

    #[test]
    fn empty_text_signature_is_all_zero() {
        let sig = signature_of("");
        assert!(sig.iter().all(|x| *x == 0));
    }

    #[test]
    fn sub_shingle_text_signature_is_all_zero() {
        let sig = signature_of("abc"); // < 5 chars
        assert!(sig.iter().all(|x| *x == 0));
    }

    #[test]
    fn identical_text_produces_identical_signatures() {
        let a = signature_of("sql injection in the login form");
        let b = signature_of("sql injection in the login form");
        assert_eq!(a, b);
    }

    #[test]
    fn signature_is_deterministic_across_calls() {
        let a = signature_of("cross site scripting in the search field");
        for _ in 0..5 {
            let b = signature_of("cross site scripting in the search field");
            assert_eq!(a, b, "MinHash must be deterministic across invocations");
        }
    }

    #[test]
    fn jaccard_of_identical_sigs_is_one() {
        let s = signature_of("alpha beta gamma delta epsilon zeta");
        assert_eq!(jaccard(&s, &s), 1.0);
    }

    #[test]
    fn jaccard_of_disjoint_texts_is_near_zero() {
        let a = signature_of("the quick brown fox jumps over the lazy dog");
        let b = signature_of("xyzzy plover frobnicate quux meaningless nonsense");
        let j = jaccard(&a, &b);
        assert!(j < 0.15, "disjoint jaccard should be near 0, got {j}");
    }

    #[test]
    fn jaccard_of_near_duplicates_is_high() {
        let a = signature_of("SQL injection in the login form");
        let b = signature_of("sql injection in the login form!!");
        let j = jaccard(&a, &b);
        assert!(
            j > 0.9,
            "near-duplicate jaccard should be very high, got {j}"
        );
    }

    #[test]
    fn jaccard_tracks_known_overlap_ratio() {
        let a = signature_of("alpha beta gamma delta epsilon. unrelated tail one here.");
        let b = signature_of("alpha beta gamma delta epsilon. different tail two here.");
        let j = jaccard(&a, &b);
        assert!(
            j > 0.4 && j < 0.95,
            "partial overlap jaccard out of band: {j}"
        );
    }

    #[test]
    fn fixture_signature_pins_constants() {
        // Regression: if PERMUTATION_SEED, NUM_PERMUTATIONS, shingle size, or
        // the blake3-truncation scheme change, this test fails loudly.
        let sig = signature_of("fastrag minhash fixture text v1");
        let expected_first_four: [u64; 4] = FIXTURE_FIRST_FOUR;
        assert_eq!(
            &sig[..4],
            &expected_first_four[..],
            "fixture signature changed — did you change PERMUTATION_SEED, \
             NUM_PERMUTATIONS, SHINGLE_SIZE, or the hash construction? \
             If the change was intentional, regenerate by printing sig[..4] \
             and pasting the new values."
        );
    }

    // Bootstrapped fixture. Changing PERMUTATION_SEED, NUM_PERMUTATIONS,
    // SHINGLE_SIZE, or the hash construction invalidates this.
    const FIXTURE_FIRST_FOUR: [u64; 4] = [
        72568055072512618,
        90035311216966048,
        19262101806995001,
        143955997002931563,
    ];
}
