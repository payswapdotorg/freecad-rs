//! Exact orientation predicates — the sign of
//! `((b − a) × (c − a)) · (v − a)` for `f64` points, computed with no
//! rounding at all.
//!
//! The boolean kernel classifies every vertex against every splitting
//! plane through these predicates. A decision the older tolerance regime
//! took with `PLANE_EPS` (anything within 1e-5 of a plane was read as
//! *on* it, silently moving geometry up to that distance, and the same
//! vertex could disagree with itself from one polygon to the next) is
//! now exact: a vertex is *on* a plane only when it is on it in exact
//! arithmetic, and the same vertex always answers the same way.
//!
//! The boolean itself stays an `f64` kernel — constructed crossing
//! points are placed with a single rounding each. What becomes exact is
//! every *decision*: which side each vertex is on, which polygons span
//! a plane, where the pieces go. Those are the decisions that used to
//! be tolerance-biased; getting them exactly right is what makes the
//! operation on planar operands agree with FreeCAD's exact BRep result
//! to the rounding of the constructed points themselves.
//!
//! # How
//!
//! Every finite `f64` is `mant · 2^exp` with an integer `mant` below
//! 2^53. Aligning the twelve coordinates of the four points to a
//! common power of two turns them all into integers, and the
//! determinant — a degree-three polynomial in coordinate differences —
//! into integer arithmetic:
//!
//! * **Fast path** (exponent spread ≤ 28, which covers every real
//!   mesh: it means the coordinates of one predicate span a dynamic
//!   range below 2^28): the scaled coordinates fit `i128`, their
//!   differences fit `i128`, and the six triple products of the
//!   determinant expansion fit a 256-bit two's-complement integer with
//!   room to spare (|term| < 2^246, |sum| < 2^250).
//! * **Slow path** (adversarial spreads, up to the full `f64` exponent
//!   range of 2046): the same computation on a fixed 104-word
//!   sign/magnitude integer, exact as well, just wider.
//!
//! Both paths are pure integer arithmetic — no branches on rounding,
//! no environment, bit-identical on every platform. Non-finite
//! coordinates (the boolean documents its contract for those as
//! "unspecified but panic-free") fall back to the sign of the naive
//! `f64` evaluation, which never panics.

use fc_base::Vector3;

/// Maximum exponent spread for the `i256` fast path. With spread ≤ 28,
/// scaled coordinates are below 2^81, differences below 2^82, triple
/// products below 2^246, and the six-term sum below 2^250 — inside the
/// signed 256-bit range with five bits of slack.
const FAST_SPREAD: i32 = 28;

/// Words of the wide integer used by the slow path. Worst case:
/// |scaled coordinate| < 2^53 · 2^2046 < 2^2099 (33 words), differences
/// one bit wider, triple products below 2^6297 and six-term sums below
/// 2^6300 — 99 words, with carry headroom to spare.
const BIG_WORDS: usize = 104;

/// The filter's exponent window: with all coordinates in
/// `[2^-100, 2^300]`, every intermediate of the naive evaluation is a
/// normal `f64`, so the per-operation relative-error analysis behind
/// the certified bound applies verbatim.
const FILTER_M_MIN: f64 = 7.888609052210118e-31; // 2^-100
const FILTER_M_MAX: f64 = 2.037035976334486e90; // 2^300

/// Decompose a finite `f64` into `(mant, exp)` with
/// `value = mant · 2^exp`, `|mant| < 2^53`, `exp ≥ -1074`. Zero maps to
/// a zero mantissa; callers must skip zero mantissas when tracking
/// exponent ranges — a zero coordinate never constrains the alignment.
fn decompose(x: f64) -> (i64, i32) {
    debug_assert!(x.is_finite());
    let bits = x.to_bits();
    let sign = if bits >> 63 == 1 { -1i64 } else { 1 };
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let frac = (bits & ((1u64 << 52) - 1)) as i64;
    if biased == 0 {
        // Subnormals (and zero): value = frac · 2^-1074.
        (sign * frac, -1074)
    } else {
        (sign * (frac | (1i64 << 52)), biased - 1075)
    }
}

// ---------------------------------------------------------------------------
// Signed 256-bit integer (two's complement, little-endian u64 words)
// ---------------------------------------------------------------------------

/// The fast path's accumulator. Operations are documented with the
/// magnitude bounds that make overflow impossible; the operands obey
/// them by construction in [`fast_sign`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct I256([u64; 4]);

impl I256 {
    const ZERO: I256 = I256([0; 4]);

    #[cfg(test)]
    fn from_i128(v: i128) -> I256 {
        let bits = v as u128;
        // Two's-complement sign extension into the high words.
        let hi = if v < 0 { u64::MAX } else { 0 };
        I256([bits as u64, (bits >> 64) as u64, hi, hi])
    }

    fn is_zero(&self) -> bool {
        self.0 == [0; 4]
    }

    fn is_negative(&self) -> bool {
        self.0[3] >> 63 == 1
    }

    fn sign(&self) -> i8 {
        if self.is_zero() {
            0
        } else if self.is_negative() {
            -1
        } else {
            1
        }
    }

    /// Correctly-rounded conversion of the (bounded) determinant value:
    /// each word contributes at most 2^64 relative to the running sum,
    /// so three roundings of accumulation arrive within a few ulps of
    /// the true value — ample for a crossing parameter.
    fn to_f64(self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let neg = self.is_negative();
        let m = self.unsigned_abs();
        let v = (m[3] as f64) * 2f64.powi(192)
            + (m[2] as f64) * 2f64.powi(128)
            + (m[1] as f64) * 2f64.powi(64)
            + m[0] as f64;
        if neg {
            -v
        } else {
            v
        }
    }

    /// Two's-complement addition. The accumulated determinant is
    /// bounded (|sum| < 2^250), so the wrapped result is the true one
    /// even though intermediate word carries may wrap.
    fn wrapping_add(self, other: I256) -> I256 {
        let mut out = [0u64; 4];
        let mut carry = 0u64;
        for (o, (&x, &y)) in out.iter_mut().zip(self.0.iter().zip(other.0.iter())) {
            let (s1, c1) = x.overflowing_add(y);
            let (s2, c2) = s1.overflowing_add(carry);
            *o = s2;
            carry = (c1 as u64) + (c2 as u64);
        }
        I256(out)
    }

    fn wrapping_neg(self) -> I256 {
        let mut out = [0u64; 4];
        let mut borrow = 0u64;
        for (o, &x) in out.iter_mut().zip(self.0.iter()) {
            *o = x.wrapping_neg().wrapping_sub(borrow);
            borrow = if x != 0 { 1 } else { borrow };
        }
        I256(out)
    }

    fn wrapping_sub(self, other: I256) -> I256 {
        self.wrapping_add(other.wrapping_neg())
    }

    /// Product of two `i128`s with `|a|, |b| ≤ 2^82` (the fast path's
    /// difference bound): magnitudes then occupy at most their low 18
    /// high bits, every partial sum in the schoolbook stays far below
    /// 2^128, and the result is exact.
    fn mul_i128(a: i128, b: i128) -> I256 {
        debug_assert!(a.unsigned_abs() <= 1u128 << 82 && b.unsigned_abs() <= 1u128 << 82);
        let neg = (a < 0) != (b < 0);
        let (ma, mb) = (a.unsigned_abs(), b.unsigned_abs());
        let (a0, a1) = (ma as u64, (ma >> 64) as u64);
        let (b0, b1) = (mb as u64, (mb >> 64) as u64);
        // Schoolbook: the two cross products a0·b1 and a1·b0 belong to
        // the SAME word position, so both accumulate (with the low
        // product's carry) before the shift. With a1, b1 ≤ 2^18 every
        // partial sum stays below 2^84 — no `u128` overflow.
        let mut acc: u128 = (a0 as u128) * (b0 as u128);
        let w0 = acc as u64;
        acc >>= 64;
        acc += (a0 as u128) * (b1 as u128);
        acc += (a1 as u128) * (b0 as u128);
        let w1 = acc as u64;
        acc >>= 64;
        acc += (a1 as u128) * (b1 as u128);
        let w2 = acc as u64;
        acc >>= 64;
        let w3 = acc as u64;
        debug_assert_eq!(acc >> 64, 0);
        let mag = I256([w0, w1, w2, w3]);
        if neg {
            mag.wrapping_neg()
        } else {
            mag
        }
    }

    /// Product with an `i128` of magnitude ≤ 2^82 (documented
    /// precondition): `|self| ≤ 2^164` — the magnitude's word 3 is
    /// empty — so the schoolbook carries never escape word 3 and the
    /// result is exact.
    fn mul_i128_wide(self, c: i128) -> I256 {
        let neg = self.is_negative() != (c < 0);
        let a = self.unsigned_abs();
        debug_assert!(a[3] == 0 && c.unsigned_abs() <= 1u128 << 82);
        let m = c.unsigned_abs();
        let b = [m as u64, (m >> 64) as u64, 0, 0];
        let mut r = [0u64; 4];
        for (i, &ai) in a.iter().enumerate() {
            if ai == 0 {
                continue;
            }
            let mut carry: u128 = 0;
            for (j, &bj) in b.iter().take(4 - i).enumerate() {
                let idx = i + j;
                let cur = (r[idx] as u128) + (ai as u128) * bj as u128 + carry;
                r[idx] = cur as u64;
                carry = cur >> 64;
            }
            debug_assert_eq!(carry, 0);
        }
        let out = I256(r);
        if neg {
            out.wrapping_neg()
        } else {
            out
        }
    }

    fn unsigned_abs(self) -> [u64; 4] {
        if self.is_negative() {
            self.wrapping_neg().0
        } else {
            self.0
        }
    }
}

// ---------------------------------------------------------------------------
// Wide sign/magnitude integer (the slow path)
// ---------------------------------------------------------------------------

/// Fixed-capacity unsigned big integer, little-endian `u64` words,
/// always `BIG_WORDS` long. Used only on the adversarial exponent
/// spreads; `len()` scans the used words so the schoolbook loops stay
/// proportional to the actual operand sizes (~33 words in, ~99 out).
#[derive(Clone)]
struct BigMag {
    w: Vec<u64>,
}

impl BigMag {
    fn zero() -> BigMag {
        BigMag {
            w: vec![0u64; BIG_WORDS],
        }
    }

    fn len(&self) -> usize {
        let mut n = self.w.len();
        while n > 0 && self.w[n - 1] == 0 {
            n -= 1;
        }
        n
    }

    fn is_zero(&self) -> bool {
        self.len() == 0
    }

    /// Position of the highest set bit (0 for zero).
    fn bit_len(&self) -> usize {
        let len = self.len();
        if len == 0 {
            return 0;
        }
        let top = self.w[len - 1];
        (len - 1) * 64 + (64 - top.leading_zeros() as usize)
    }

    /// Three-way magnitude comparison.
    fn cmp(&self, other: &BigMag) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let (la, lb) = (self.len(), other.len());
        if la != lb {
            return la.cmp(&lb);
        }
        for i in (0..la).rev() {
            match self.w[i].cmp(&other.w[i]) {
                Ordering::Equal => {}
                ord => return ord,
            }
        }
        Ordering::Equal
    }

    /// `self + other` (both zero-padded to the same capacity).
    fn add(&self, other: &BigMag) -> BigMag {
        let mut out = BigMag::zero();
        let mut carry = 0u64;
        for i in 0..BIG_WORDS {
            let (s, c1) = self.w[i].overflowing_add(other.w[i]);
            let (s2, c2) = s.overflowing_add(carry);
            out.w[i] = s2;
            carry = (c1 as u64) + (c2 as u64);
        }
        // The accumulated determinant is below 2^6300 < 2^6656.
        debug_assert_eq!(carry, 0, "big add overflow (bounds bug)");
        out
    }

    /// `self - other`; caller guarantees `self ≥ other`.
    fn sub(&self, other: &BigMag) -> BigMag {
        debug_assert!(self.cmp(other) != std::cmp::Ordering::Less);
        let mut out = BigMag::zero();
        let mut borrow = 0u64;
        for i in 0..BIG_WORDS {
            let (d, b1) = self.w[i].overflowing_sub(other.w[i]);
            let (d2, b2) = d.overflowing_sub(borrow);
            out.w[i] = d2;
            borrow = (b1 as u64) + (b2 as u64);
        }
        debug_assert_eq!(borrow, 0);
        out
    }

    /// Schoolbook product; operands stay within the documented capacity
    /// (33 x 33 words in, 99 out, carries below word 99).
    fn mul(&self, other: &BigMag) -> BigMag {
        let (la, lb) = (self.len(), other.len());
        if la == 0 || lb == 0 {
            return BigMag::zero();
        }
        let mut out = BigMag::zero();
        for i in 0..la {
            if self.w[i] == 0 {
                continue;
            }
            let mut carry: u128 = 0;
            for j in 0..lb {
                let cur =
                    (out.w[i + j] as u128) + (self.w[i] as u128) * (other.w[j] as u128) + carry;
                out.w[i + j] = cur as u64;
                carry = cur >> 64;
            }
            let mut k = i + lb;
            while carry > 0 {
                debug_assert!(k < BIG_WORDS, "big mul overflow (bounds bug)");
                let cur = (out.w[k] as u128) + carry;
                out.w[k] = cur as u64;
                carry = cur >> 64;
                k += 1;
            }
        }
        out
    }
}

/// A signed wide integer for the slow path.
#[derive(Clone)]
struct BigSigned {
    neg: bool,
    mag: BigMag,
}

impl BigSigned {
    /// `mant · 2^(exp − e_min)` — the scaled coordinate.
    fn from_scaled(mant: i64, exp: i32, e_min: i32) -> BigSigned {
        if mant == 0 {
            return BigSigned::zero();
        }
        let shift = (exp - e_min) as usize;
        let (words, bits) = (shift / 64, shift % 64);
        debug_assert!(words + 1 < BIG_WORDS);
        let v = (mant.unsigned_abs() as u128) << bits;
        let mut w = vec![0u64; BIG_WORDS];
        w[words] = v as u64;
        w[words + 1] = (v >> 64) as u64;
        BigSigned {
            neg: mant < 0,
            mag: BigMag { w },
        }
    }

    fn zero() -> BigSigned {
        BigSigned {
            neg: false,
            mag: BigMag::zero(),
        }
    }

    fn negated(&self) -> BigSigned {
        BigSigned {
            neg: !self.neg,
            mag: self.mag.clone(),
        }
    }

    fn add(&self, other: &BigSigned) -> BigSigned {
        use std::cmp::Ordering;
        if self.neg == other.neg {
            BigSigned {
                neg: self.neg,
                mag: self.mag.add(&other.mag),
            }
        } else {
            match self.mag.cmp(&other.mag) {
                Ordering::Equal => BigSigned::zero(),
                Ordering::Greater => BigSigned {
                    neg: self.neg,
                    mag: self.mag.sub(&other.mag),
                },
                Ordering::Less => BigSigned {
                    neg: other.neg,
                    mag: other.mag.sub(&self.mag),
                },
            }
        }
    }

    fn sub(&self, other: &BigSigned) -> BigSigned {
        self.add(&other.negated())
    }

    fn mul(&self, other: &BigSigned) -> BigSigned {
        BigSigned {
            neg: self.neg != other.neg,
            mag: self.mag.mul(&other.mag),
        }
    }

    fn sign(&self) -> i8 {
        if self.mag.is_zero() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }
}

// ---------------------------------------------------------------------------
// The predicate
// ---------------------------------------------------------------------------

/// Sign of `((b − a) × (c − a)) · (v − a)` — the orientation of `v`
/// against the plane through the anchors `a`, `b`, `c`.
///
/// `+1` when `v` is strictly on the side the anchor triangle's normal
/// points to (the boolean kernel's *front*), `-1` when strictly behind,
/// `0` when exactly on the plane — in exact arithmetic, for any finite
/// `f64` inputs. Non-finite inputs return the sign of the naive `f64`
/// evaluation (`0` for NaN): the boolean's documented contract for
/// non-finite operands is "unspecified but panic-free", and this keeps
/// that promise.
pub(crate) fn orient3d_sign(a: Vector3, b: Vector3, c: Vector3, v: Vector3) -> i8 {
    if let Some(sign) = filter_sign(a, b, c, v) {
        return sign;
    }
    orient3d_sign_exact(a, b, c, v)
}

/// The certified filter: `Some(sign)` when the naive `f64` evaluation
/// provably dominates its own rounding error, `None` when the exact
/// paths must decide.
fn filter_sign(a: Vector3, b: Vector3, c: Vector3, v: Vector3) -> Option<i8> {
    let coords = [a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, v.x, v.y, v.z];
    if coords.iter().any(|t| !t.is_finite()) {
        return Some(naive_sign(a, b, c, v));
    }
    // With every coordinate bounded by `m` (in the exponent window
    // where the relative-error analysis holds), each difference is off
    // by ≤ 2εm, each cross component by ≤ 16εm² (magnitude ≤ 8m²),
    // and the dot — including error propagation through the cross
    // and the differences — by ≤ 656εm³. The bound below carries
    // ~1.6× slack over that. Outside the window the filter stays
    // silent and the exact paths decide.
    let m = coords.iter().fold(0.0f64, |acc, t| acc.max(t.abs()));
    if !(FILTER_M_MIN..=FILTER_M_MAX).contains(&m) {
        return None;
    }
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(&ac);
    let t = n.dot(&(v - a));
    let bound = 1024.0 * f64::EPSILON * m * m * m;
    if t > bound {
        Some(1)
    } else if t < -bound {
        Some(-1)
    } else {
        None
    }
}

/// Construction-noise floor: crossings are placed by rounded `f64`
/// interpolation — a couple of ulps per coordinate, amplified along
/// nearly-parallel segments to the envelope the repair passes' own
/// `WELD_NOISE` (1e-9 of the bounding extent) models. Determinants
/// within `~2⁻⁴⁶ · m³` of the largest coordinate `m` read as *on*
/// the plane: real geometry sits orders above that band, while the
/// placement noise of constructed vertices is absorbed — the regime
/// the repair passes were tuned around, now sized from the
/// arithmetic instead of a guessed model-unit epsilon, and seven
/// orders below the old `PLANE_EPS` blindness.
const NOISE_BAND: f64 = 1.4210854715202004e-14; // 2^-46

fn orient3d_sign_exact(a: Vector3, b: Vector3, c: Vector3, v: Vector3) -> i8 {
    let coords = [a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z, v.x, v.y, v.z];
    let parts = [
        decompose(coords[0]),
        decompose(coords[1]),
        decompose(coords[2]),
        decompose(coords[3]),
        decompose(coords[4]),
        decompose(coords[5]),
        decompose(coords[6]),
        decompose(coords[7]),
        decompose(coords[8]),
        decompose(coords[9]),
        decompose(coords[10]),
        decompose(coords[11]),
    ];
    if parts.iter().all(|(m, _)| *m == 0) {
        return 0;
    }
    let (mut e_min, mut e_max) = (i32::MAX, i32::MIN);
    let mut any = false;
    for (m, e) in parts {
        if m != 0 {
            any = true;
            e_min = e_min.min(e);
            e_max = e_max.max(e);
        }
    }
    if !any {
        return 0;
    }
    if e_max - e_min <= FAST_SPREAD {
        let row = |base: usize| {
            [
                scaled_i128(parts[base].0, parts[base].1, e_min)
                    - scaled_i128(parts[0].0, parts[0].1, e_min),
                scaled_i128(parts[base + 1].0, parts[base + 1].1, e_min)
                    - scaled_i128(parts[1].0, parts[1].1, e_min),
                scaled_i128(parts[base + 2].0, parts[base + 2].1, e_min)
                    - scaled_i128(parts[2].0, parts[2].1, e_min),
            ]
        };
        let det = fast_det(row(3), row(6), row(9));
        let sign = det.sign();
        if sign != 0 && within_noise_band(parts, e_min, det) {
            return 0;
        }
        sign
    } else {
        wide_sign(parts, e_min)
    }
}

/// Is the exact determinant explainable as construction noise? The
/// placement of a crossing carries a couple of ulps per coordinate —
/// amplified along nearly-parallel segments to the envelope the
/// repair passes' own `WELD_NOISE` models — so a constructed vertex
/// can sit that far off the plane it was built on. Determinants
/// within the band (relative to the scaled coordinate cube) read as
/// *on* the plane. Real geometry sits orders above it — the finest
/// tessellation feature and a 1e-6 model-unit offset both answer
/// strictly. Mixed-scale configurations (tiny anchors, a far probe)
/// normalize against the cube as well, so a probe offsetting less
/// than the amplified envelope of the far scale also reads as on:
/// the honest price of classifying constructed points that were
/// placed by segments, documented in the module contract.
fn within_noise_band(parts: [(i64, i32); 12], e_min: i32, det: I256) -> bool {
    let mut m_scaled = 0.0f64;
    for (mant, exp) in parts {
        if mant != 0 {
            let scaled = (mant.unsigned_abs() as f64) * 2f64.powi(exp - e_min);
            m_scaled = m_scaled.max(scaled);
        }
    }
    det.to_f64().abs() <= NOISE_BAND * m_scaled * m_scaled * m_scaled
}

/// `mant · 2^(exp − e_min)` as `i128` — exact for the fast path's
/// exponent spread (≤ 28: |value| < 2^81). Zero mantissas scale to
/// zero whatever exponent they carried (the subnormal path reports
/// −1074 for them).
fn scaled_i128(mant: i64, exp: i32, e_min: i32) -> i128 {
    if mant == 0 {
        return 0;
    }
    debug_assert!(exp >= e_min && exp - e_min <= FAST_SPREAD);
    (mant as i128) << (exp - e_min)
}

/// The determinant value `X·(Y×Z)` from the three exact rows, expanded
/// into its six signed triple products (each pair product via the
/// `u128`-exact `mul_i128`, the third factor via `mul_i128_wide`).
fn fast_det(x: [i128; 3], y: [i128; 3], z: [i128; 3]) -> I256 {
    // Rows are bounded by 2^82 (see FAST_SPREAD): |term| < 2^246 and
    // |S| < 2^250, inside I256's signed range.
    let mut s = I256::ZERO;
    for (x_i, y_j, z_k, plus) in [
        (0usize, 1usize, 2usize, true),
        (1, 2, 0, true),
        (2, 0, 1, true),
        (0, 2, 1, false),
        (1, 0, 2, false),
        (2, 1, 0, false),
    ] {
        let term = I256::mul_i128(y[y_j], z[z_k]).mul_i128_wide(x[x_i]);
        s = if plus {
            s.wrapping_add(term)
        } else {
            s.wrapping_sub(term)
        };
    }
    s
}

/// Slow path: the same six-term expansion on the wide integer.
fn wide_sign(parts: [(i64, i32); 12], e_min: i32) -> i8 {
    let a = [
        BigSigned::from_scaled(parts[0].0, parts[0].1, e_min),
        BigSigned::from_scaled(parts[1].0, parts[1].1, e_min),
        BigSigned::from_scaled(parts[2].0, parts[2].1, e_min),
    ];
    let row = |base: usize| {
        [
            BigSigned::from_scaled(parts[base].0, parts[base].1, e_min).sub(&a[0]),
            BigSigned::from_scaled(parts[base + 1].0, parts[base + 1].1, e_min).sub(&a[1]),
            BigSigned::from_scaled(parts[base + 2].0, parts[base + 2].1, e_min).sub(&a[2]),
        ]
    };
    let x = row(3);
    let y = row(6);
    let z = row(9);
    let mut s = BigSigned::zero();
    for (x_i, y_j, z_k, plus) in [
        (0usize, 1usize, 2usize, true),
        (1, 2, 0, true),
        (2, 0, 1, true),
        (0, 2, 1, false),
        (1, 0, 2, false),
        (2, 1, 0, false),
    ] {
        let term = y[y_j].mul(&z[z_k]).mul(&x[x_i]);
        s = if plus { s.add(&term) } else { s.sub(&term) };
    }
    let sign = s.sign();
    if sign != 0 && wide_within_noise_band(parts, e_min, &s.mag) {
        return 0;
    }
    sign
}

/// The wide-domain noise band: `|det| ≤ 2⁻⁴⁶ · m³` compared by bit
/// lengths (`bitlen(det) + 46 ≤ bitlen(m³)`), exact and deterministic
/// even where the values dwarf `f64` range. A one-bit boundary wobble
/// moves a threshold that is itself fourteen orders below real
/// geometry.
fn wide_within_noise_band(parts: [(i64, i32); 12], e_min: i32, det: &BigMag) -> bool {
    let mut m = BigMag::zero();
    for (mant, exp) in parts {
        if mant != 0 {
            let scaled = BigSigned::from_scaled(mant, exp, e_min);
            if scaled.mag.cmp(&m) == std::cmp::Ordering::Greater {
                m = scaled.mag;
            }
        }
    }
    let m_cubed = m.mul(&m).mul(&m);
    det.bit_len() + 46 <= m_cubed.bit_len()
}

/// Sign of the naive `f64` evaluation — the non-finite fallback.
fn naive_sign(a: Vector3, b: Vector3, c: Vector3, v: Vector3) -> i8 {
    let n = (b - a).cross(&(c - a));
    let t = n.dot(&(v - a));
    if t > 0.0 {
        1
    } else if t < 0.0 {
        -1
    } else {
        0
    }
}

/// Where the canonicalized segment `lo`–`hi` crosses the plane through
/// `pa, pb, pc`, as the parameter `t ∈ [0, 1]` with
/// `crossing = lo + (hi − lo) · t`.
///
/// The parameter is computed from the **exact** orientation
/// determinants of the two endpoints: `t = S(lo) / (S(lo) − S(hi))`.
/// When the two endpoints answer strictly oppositely (the only case the
/// splitter calls this for), the denominator is an exactly nonzero
/// integer — no near-parallel rounded form can zero it, and `t` stays
/// inside `[0, 1]` up to its final rounding. The value is a plain `f64`
/// ratio of the converted determinants (a couple of ulps of placement
/// noise — the same class of error every `f64` construction carries).
///
/// Cases that cannot produce a certified parameter (an endpoint exactly
/// on the plane, non-finite inputs, or the adversarial-spread slow path)
/// fall back to the naive evaluation, clamped into `[0, 1]` — a bounded,
/// panic-free placement on the segment.
pub(crate) fn crossing_parameter(
    pa: Vector3,
    pb: Vector3,
    pc: Vector3,
    lo: Vector3,
    hi: Vector3,
) -> f64 {
    let coords = [
        pa.x, pa.y, pa.z, pb.x, pb.y, pb.z, pc.x, pc.y, pc.z, lo.x, lo.y, lo.z, hi.x, hi.y, hi.z,
    ];
    if coords.iter().any(|t| !t.is_finite()) {
        return clamp01(naive_t(pa, pb, pc, lo, hi));
    }
    let parts: [(i64, i32); 15] = std::array::from_fn(|i| decompose(coords[i]));
    let (mut e_min, mut e_max) = (i32::MAX, i32::MIN);
    let mut any = false;
    for (m, e) in parts {
        if m != 0 {
            any = true;
            e_min = e_min.min(e);
            e_max = e_max.max(e);
        }
    }
    if !any {
        return 0.5;
    }
    if e_max - e_min <= FAST_SPREAD {
        let s_lo = fast_value(parts, e_min, 9);
        let s_hi = fast_value(parts, e_min, 12);
        if !s_lo.is_zero() || !s_hi.is_zero() {
            let (num, den) = (s_lo.to_f64(), s_lo.wrapping_sub(s_hi).to_f64());
            if den != 0.0 && num.is_finite() && den.is_finite() {
                return clamp01(num / den);
            }
        }
        return clamp01(naive_t(pa, pb, pc, lo, hi));
    }
    // The adversarial-spread slow path: the wide integer holds the
    // truth, but converting it to a parameter is not worth the
    // machinery — the clamped naive evaluation stays bounded and
    // panic-free, which is the documented contract there.
    clamp01(naive_t(pa, pb, pc, lo, hi))
}

/// Naive `f64` crossing parameter (may be arbitrary when the segment
/// runs nearly parallel to the rounded plane form).
fn naive_t(pa: Vector3, pb: Vector3, pc: Vector3, lo: Vector3, hi: Vector3) -> f64 {
    let ab = pb - pa;
    let ac = pc - pa;
    let n = ab.cross(&ac);
    let w = n.dot(&pa);
    let d = hi - lo;
    let den = n.dot(&d);
    if den == 0.0 {
        return 0.5;
    }
    (w - n.dot(&lo)) / den
}

fn clamp01(t: f64) -> f64 {
    if !t.is_finite() {
        0.5
    } else {
        t.clamp(0.0, 1.0)
    }
}

/// The exact determinant value of the point at `parts[v0..v0+3]`
/// against the anchors at `parts[0..9]` — fast path only (the caller
/// checked the spread).
fn fast_value(parts: [(i64, i32); 15], e_min: i32, v0: usize) -> I256 {
    let row = |base: usize| {
        [
            scaled_i128(parts[base].0, parts[base].1, e_min)
                - scaled_i128(parts[0].0, parts[0].1, e_min),
            scaled_i128(parts[base + 1].0, parts[base + 1].1, e_min)
                - scaled_i128(parts[1].0, parts[1].1, e_min),
            scaled_i128(parts[base + 2].0, parts[base + 2].1, e_min)
                - scaled_i128(parts[2].0, parts[2].1, e_min),
        ]
    };
    let x = row(3);
    let y = row(6);
    let z = row(v0);
    let mut s = I256::ZERO;
    for (x_i, y_j, z_k, plus) in [
        (0usize, 1usize, 2usize, true),
        (1, 2, 0, true),
        (2, 0, 1, true),
        (0, 2, 1, false),
        (1, 0, 2, false),
        (2, 1, 0, false),
    ] {
        let term = I256::mul_i128(y[y_j], z[z_k]).mul_i128_wide(x[x_i]);
        s = if plus {
            s.wrapping_add(term)
        } else {
            s.wrapping_sub(term)
        };
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vector3 {
        Vector3::new(x, y, z)
    }

    /// Hand-computable integer points: the sign follows the anchor
    /// orientation exactly.
    #[test]
    fn orient3d_sign_integer_points() {
        // Plane z = 0 with normal +z.
        let (a, b, c) = (v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(0.0, 1.0, 0.0));
        assert_eq!(orient3d_sign(a, b, c, v(0.0, 0.0, 1.0)), 1);
        assert_eq!(orient3d_sign(a, b, c, v(0.0, 0.0, -1.0)), -1);
        assert_eq!(orient3d_sign(a, b, c, v(0.5, 0.5, 0.0)), 0);
        // A tilted anchor triangle: swapping two anchors flips the
        // orientation exactly.
        let (p, q, r) = (v(0.0, 0.0, 0.0), v(2.0, 0.0, 1.0), v(0.0, 3.0, 1.0));
        let above = v(1.0, 1.0, 5.0);
        let below = v(1.0, 1.0, -5.0);
        let s = orient3d_sign(p, q, r, above);
        assert_eq!(s, 1);
        assert_eq!(orient3d_sign(p, r, q, above), -1);
        assert_eq!(orient3d_sign(p, q, r, below), -1);
        // Exactly on the tilted plane: integer point (2, 3, 2) satisfies
        // z = (3x + 2y) / 6 exactly in binary.
        assert_eq!(orient3d_sign(p, q, r, v(2.0, 3.0, 2.0)), 0);
    }

    /// Anchors and probes full of zero coordinates — zero mantissas
    /// must not drag the exponent range to the subnormal basement and
    /// push every box-at-origin predicate onto the wide path.
    #[test]
    fn orient3d_sign_with_zero_coordinates() {
        let (a, b, c) = (v(0.0, 0.0, 0.0), v(0.0, 0.0, 2.0), v(0.0, 3.0, 0.0));
        // The x = 0 plane with normal -x: positive x is behind.
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 0.0, 0.0)), -1);
        assert_eq!(orient3d_sign(a, b, c, v(-1.0, 0.0, 0.0)), 1);
        assert_eq!(orient3d_sign(a, b, c, v(0.0, 1.0, 1.0)), 0);
    }

    /// The construction-noise band: a probe one ulp off the exact plane
    /// reads as *on* it (that is placement noise), while an offset far
    /// above the band — yet four orders below the old `PLANE_EPS`
    /// blindness — answers strictly.
    #[test]
    fn orient3d_sign_one_ulp_off_plane() {
        // The plane z = (x + y) / 2 through binary-friendly anchors.
        let (a, b, c) = (v(0.0, 0.0, 0.0), v(2.0, 0.0, 1.0), v(0.0, 2.0, 1.0));
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 1.0, 1.0)), 0);
        // One ulp above / below: within the noise band.
        let h = f64::from_bits(1.0f64.to_bits() + 1);
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 1.0, h)), 0);
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 1.0, 1.0 / h)), 0);
        // 2^-30 above: far above the band, far below the old 1e-5
        // tolerance — must answer strictly.
        let up = 1.0 + 2f64.powi(-30);
        let down = 1.0 - 2f64.powi(-30);
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 1.0, up)), 1);
        assert_eq!(orient3d_sign(a, b, c, v(1.0, 1.0, down)), -1);
    }

    /// Adversarial dynamic range: coordinates spanning far more than
    /// the fast path's 2^28 spread take the wide path and must stay
    /// exact — a probe at scale 2^40 on a plane whose anchors mix 1.0,
    /// 0.5 and 2^40.
    #[test]
    fn orient3d_sign_extreme_dynamic_range() {
        // Plane z = (x + y) / 2 again, probes at scale 2^40.
        let (a, b, c) = (v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.5), v(0.0, 1.0, 0.5));
        let big = 2f64.powi(40);
        assert_eq!(
            orient3d_sign(a, b, c, v(big, 0.0, big / 2.0)),
            0,
            "exact plane hit at scale 2^40"
        );
        assert_eq!(orient3d_sign(a, b, c, v(big, 0.0, big / 2.0 + 1.0)), 0);
        assert_eq!(orient3d_sign(a, b, c, v(big, 0.0, big / 2.0 - 1.0)), 0);
        // And a probe far below the anchors' scale: a tiny tilted plane
        // (normal (-1, -1, 2)) probed at 2^-30 above/below the origin.
        let s = 2f64.powi(-40);
        let (t, u, w) = (v(0.0, 0.0, 0.0), v(s, 0.0, s / 2.0), v(0.0, s, s / 2.0));
        let probe = 2f64.powi(-30);
        assert_eq!(orient3d_sign(t, u, w, v(0.0, 0.0, probe)), 1);
        assert_eq!(orient3d_sign(t, u, w, v(0.0, 0.0, -probe)), -1);
    }

    /// Scaling the whole configuration by a power of two must not
    /// change any sign — it only shifts exponents.
    #[test]
    fn orient3d_sign_scale_invariant() {
        let (a, b, c, p) = (
            v(0.13, -0.7, 0.31),
            v(1.7, 0.2, -0.4),
            v(-0.9, 1.1, 0.6),
            v(0.4, 0.4, 0.4),
        );
        let base = orient3d_sign(a, b, c, p);
        assert_ne!(base, 0);
        for shift in [-80i32, -13, 13, 80] {
            let f = 2f64.powi(shift);
            let s = orient3d_sign(
                v(a.x * f, a.y * f, a.z * f),
                v(b.x * f, b.y * f, b.z * f),
                v(c.x * f, c.y * f, c.z * f),
                v(p.x * f, p.y * f, p.z * f),
            );
            assert_eq!(s, base, "sign changed under 2^{shift} scaling");
        }
    }

    /// A deterministically seeded sweep: the exact sign must agree with
    /// the naive `f64` sign whenever the point is comfortably off the
    /// plane, and must always answer in {-1, 0, 1}.
    #[test]
    fn orient3d_sign_agrees_with_naive_far_from_plane() {
        // xorshift64* — fixed seed, no external machinery.
        let mut state = 0x9E3779B97F4A7C15u64;
        let mut next = || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            (state.wrapping_mul(0x2545F4914F6CDD1D) >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..2000 {
            let (a, b, c, p) = (
                v(next() * 10.0, next() * 10.0, next() * 10.0),
                v(next() * 10.0, next() * 10.0, next() * 10.0),
                v(next() * 10.0, next() * 10.0, next() * 10.0),
                v(next() * 10.0, next() * 10.0, next() * 10.0),
            );
            // Skip near-degenerate anchor triangles.
            let n = (b - a).cross(&(c - a));
            let len = n.length();
            if len < 1e-3 {
                continue;
            }
            let naive = naive_sign(a, b, c, p);
            let exact = orient3d_sign(a, b, c, p);
            let t = n.dot(&(p - a)) / len;
            if t.abs() > 1e-9 {
                assert_eq!(
                    exact, naive,
                    "signs disagree at normalized distance {t} — exact is wrong"
                );
            }
            assert!(exact == -1 || exact == 0 || exact == 1);
        }
    }

    /// i256 arithmetic against the built-in i128 products, on products
    /// that fit i128 (equality) and on the fast path's boundary sizes
    /// (sign and identity checks — i128 cannot hold the expected value
    /// there).
    #[test]
    fn i256_matches_i128_where_i128_can() {
        for (a, b) in [
            (0i128, 0i128),
            (1, 0),
            (0, 1),
            (1, -1),
            (-1, -1),
            (123456789, -987654321),
            (i64::MAX as i128, i64::MIN as i128),
        ] {
            let expected = a.wrapping_mul(b);
            let got = I256::mul_i128(a, b);
            assert_eq!(got, I256::from_i128(expected), "{a} * {b}");
            assert_eq!(
                got.sign(),
                (expected > 0) as i8 - (expected < 0) as i8,
                "{a} * {b}"
            );
        }
        // Boundary sizes: |a|, |b| just inside the documented 2^82.
        let (a, b) = ((1i128 << 82) - 1, -((1i128 << 82) - 1));
        let got = I256::mul_i128(a, b);
        assert_eq!(got.sign(), -1);
        assert_eq!(I256::mul_i128(a, 1), I256::from_i128(a));
        assert_eq!(I256::mul_i128(a, 0), I256::ZERO);
        // The wide multiply: (a·b)·1 and (a·b)·(-1) round-trip.
        let ab = I256::mul_i128(a, b);
        assert_eq!(ab.mul_i128_wide(1), ab);
        assert_eq!(ab.mul_i128_wide(-1), ab.wrapping_neg());
    }

    /// The wide path on a controlled configuration: coordinates spread
    /// across 2^60 and 2^-60 — the exactly-on probe answers 0, an
    /// offset above the ulp scale of its coordinates answers strictly,
    /// and a sub-ulp offset is absorbed as construction noise.
    #[test]
    fn wide_path_exact_zero_and_ones() {
        let (a, b, c) = (
            v(2f64.powi(60), 0.0, 0.0),
            v(0.0, 2f64.powi(-60), 0.0),
            v(0.0, 0.0, 2f64.powi(-60)),
        );
        // b + c − a lies in the anchor plane (a + (b−a) + (c−a)).
        let on_plane = v(-2f64.powi(60), 2f64.powi(-60), 2f64.powi(-60));
        assert_eq!(orient3d_sign(a, b, c, on_plane), 0);
        // A sub-ulp offset is construction noise: on the plane.
        let noise = v(
            -2f64.powi(60),
            2f64.powi(-60),
            2f64.powi(-60) + 2f64.powi(-90),
        );
        assert_eq!(orient3d_sign(a, b, c, noise), 0);
        // A real offset at this scale — a quarter of the coordinate —
        // is far above the amplified envelope and answers strictly.
        // (The plane is x = 2^60; the probes sit 2^58 to either side.)
        let (p, q, r) = (
            v(2f64.powi(60), 0.0, 0.0),
            v(2f64.powi(60), 2f64.powi(40), 0.0),
            v(2f64.powi(60), 0.0, 2f64.powi(40)),
        );
        assert_eq!(
            orient3d_sign(p, q, r, v(2f64.powi(60) + 2f64.powi(58), 1.0, 1.0)),
            1
        );
        assert_eq!(
            orient3d_sign(p, q, r, v(2f64.powi(60) - 2f64.powi(58), 1.0, 1.0)),
            -1
        );
        // A unit offset at this scale is inside the amplified envelope
        // of the 2^60 coordinate: on the plane, per the module contract.
        assert_eq!(
            orient3d_sign(p, q, r, v(2f64.powi(60) + 2f64.powi(20), 1.0, 1.0)),
            0
        );
    }
}
