//! Approximate quantiles via a t-digest.
//!
//! Unlike [`crate::aggregates::Sum`], [`crate::aggregates::Count`], [`crate::aggregates::Min`],
//! [`crate::aggregates::Max`], and [`crate::aggregates::Average`] — all of which are *exact* —
//! [`TDigest`] is an approximate, compressing sketch. Its [`Monoid`] laws hold only up to the
//! sketch's error bound, not bit-for-bit:
//!
//! - **Identity** holds exactly: an empty digest merged with `x` is equivalent to `x` (same
//!   `quantile()` answers), because the identity element carries no centroids.
//! - **Associativity** and **commutativity** hold *approximately*: `a.merge(&b).merge(&c)` and
//!   `a.merge(&b.merge(&c))` may end up with different centroid layouts, but their `quantile()`
//!   answers agree within the digest's compression-dependent error bound. Error can compound
//!   across repeated rollups (Minute→Hour→Day→Week/Month→Year), so callers that need a tight
//!   bound on a deeply-rolled-up bucket should pick a larger `compression`.
//!
//! `TDigest` is intentionally *not* generic over the measure's numeric type the way
//! [`crate::aggregates::Sum`] and [`crate::aggregates::Average`] are: centroid arithmetic is
//! always done in `f64`, since compression is meaningless for exact integer types and every
//! measure worth sketching is effectively continuous. Its `Aggregator::Input` is a plain `f64`,
//! converted from the sampled `MeasureValue` the same way every other built-in aggregate's input
//! is (see [`crate::measures::FromValue`]).
//!
//! `TDigest` is deliberately expensive relative to the other built-in aggregates (heap-allocated
//! centroid and buffer storage, O(n log n) compression) and is meant to be registered only on the
//! handful of measures that actually need quantiles — not as a blanket replacement for
//! [`crate::aggregates::Average`], which remains the cheap, exact, general-purpose mean.

use std::f64::consts::PI;

use crate::aggregator::Aggregator;
use crate::monoid::Monoid;

/// Default t-digest compression factor, used by [`TDigest::identity`] when no other value has
/// been configured.
///
/// Higher values mean more centroids, better accuracy, and a larger state. This is expected to
/// eventually be sourced from a config file rather than hardcoded; nothing about [`TDigest`]
/// itself needs to change when that happens, since `compression` is a plain field seeded once at
/// construction time, not baked into the type.
pub const DEFAULT_TDIGEST_COMPRESSION: usize = 100;

/// How many raw samples are buffered (as a multiple of `compression`) before an [`update`] or
/// [`update_in_place`] call forces a compression pass.
///
/// [`update`]: TDigest::update
/// [`update_in_place`]: TDigest::update_in_place
const BUFFER_CAPACITY_MULTIPLIER: usize = 8;

/// One "bucket" of the digest: a weighted mean summarizing `weight` raw samples.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Centroid {
    mean: f64,
    weight: u64,
}

impl Centroid {
    /// Fold `other` into `self`, producing the weighted-mean combination of both.
    fn merged_with(&self, other: &Centroid) -> Centroid {
        let weight = self.weight + other.weight;
        // Guard against the (unreachable in practice, since both weights are >= 1 whenever a
        // centroid exists) zero-weight case to avoid a division by zero.
        let mean = if weight == 0 {
            0.0
        } else {
            (self.mean * self.weight as f64 + other.mean * other.weight as f64) / weight as f64
        };
        Centroid { mean, weight }
    }
}

/// A t-digest: an approximate summary of a distribution that supports merging and quantile
/// queries.
///
/// See the [module docs](self) for how this fits into the [`Monoid`] contract.
#[derive(Debug, Clone)]
pub struct TDigest {
    /// Compressed centroids, always kept sorted by `mean`. Does not necessarily reflect every
    /// sample seen so far — see `buffer`.
    centroids: Vec<Centroid>,
    /// Raw samples not yet folded into `centroids`. Compression is deferred until the buffer
    /// fills up or an operation (`merge`, `quantile`) needs the fully-compressed view.
    buffer: Vec<f64>,
    /// This digest's compression factor. Seeded from [`DEFAULT_TDIGEST_COMPRESSION`] at
    /// construction and propagated forward through every `update`/`merge`.
    compression: usize,
    /// Exact count of every sample folded into this digest (buffered or compressed). Drives the
    /// scale function's `q = running_weight / count` term, so it is structural, not incidental.
    count: u64,
    /// Exact running minimum. Used as a read-time fallback in [`TDigest::quantile`] near `q =
    /// 0.0`, where centroid interpolation is least reliable.
    min: f64,
    /// Exact running maximum. Used as a read-time fallback in [`TDigest::quantile`] near `q =
    /// 1.0`, where centroid interpolation is least reliable.
    max: f64,
}

impl TDigest {
    /// The scale function `k(q)`, mapping a cumulative-weight fraction to "k-space".
    ///
    /// Centroids near `q = 0.5` are allowed to be fat (few, large centroids); centroids near
    /// the tails (`q` close to 0.0 or 1.0) are forced to stay thin, because quantile error is
    /// most sensitive there. This is the standard k1 scale function.
    fn k(q: f64, compression: usize) -> f64 {
        (compression as f64 / (2.0 * PI)) * (2.0 * q - 1.0).asin()
    }

    /// Merge a sorted-by-mean sequence of (possibly uncompressed) centroids down to at most
    /// `compression`-many centroids, respecting the scale function's per-region weight budget.
    ///
    /// `total_count` is the total sample weight across `sorted`, used to compute each
    /// candidate's cumulative-weight fraction `q`.
    fn compress_sorted(
        sorted: Vec<Centroid>,
        compression: usize,
        total_count: u64,
    ) -> Vec<Centroid> {
        if sorted.is_empty() || total_count == 0 {
            return Vec::new();
        }

        let mut result = Vec::with_capacity(compression.max(1));
        let mut iter = sorted.into_iter();

        // The centroid currently being grown, and the k-space position where it started.
        let mut current = iter.next().expect("checked non-empty above");
        let mut weight_before_current = 0u64;
        let mut k_start = Self::k(0.0, compression);

        for candidate in iter {
            let weight_if_merged = weight_before_current + current.weight + candidate.weight;
            let q_if_merged = weight_if_merged as f64 / total_count as f64;
            let k_if_merged = Self::k(q_if_merged.clamp(0.0, 1.0), compression);

            if k_if_merged - k_start <= 1.0 {
                // Still within budget for this centroid's region of the distribution — fold it
                // in rather than starting a new one.
                current = current.merged_with(&candidate);
            } else {
                // Budget exceeded: close out the current centroid and start a new one.
                weight_before_current += current.weight;
                result.push(current);
                let q_start = weight_before_current as f64 / total_count as f64;
                k_start = Self::k(q_start.clamp(0.0, 1.0), compression);
                current = candidate;
            }
        }
        result.push(current);
        result
    }

    /// Fold every buffered raw sample into `centroids`, then clear the buffer.
    ///
    /// A no-op if the buffer is already empty. `min`/`max`/`count` are exact and are not
    /// affected by flushing — they are maintained directly in `update_in_place`/`merge`.
    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }

        let mut all: Vec<Centroid> = self
            .centroids
            .drain(..)
            .chain(
                self.buffer
                    .drain(..)
                    .map(|mean| Centroid { mean, weight: 1 }),
            )
            .collect();
        all.sort_by(|a, b| a.mean.total_cmp(&b.mean));

        self.centroids = Self::compress_sorted(all, self.compression, self.count);
    }

    /// Recompress this digest's centroids down to `target` compression, flushing first if
    /// necessary. A no-op if already at or below `target`.
    fn recompress_to(&mut self, target: usize) {
        self.flush();
        if self.compression <= target || self.centroids.is_empty() {
            self.compression = self.compression.min(target);
            return;
        }
        let sorted = std::mem::take(&mut self.centroids);
        self.centroids = Self::compress_sorted(sorted, target, self.count);
        self.compression = target;
    }

    /// Estimate the value at quantile `q` (`0.0..=1.0`).
    ///
    /// Falls back to the exact tracked minimum/maximum at the extremes, where centroid
    /// interpolation is least reliable; otherwise linearly interpolates between the two
    /// centroids bracketing `q`.
    ///
    /// Returns `f64::NAN` if no samples have been seen yet.
    pub fn quantile(&self, q: f64) -> f64 {
        // `quantile` takes `&self`, so flushing (which needs `&mut self`) is done on a clone
        // rather than mutating in place. This keeps the read side pure at the cost of an extra
        // flush if the buffer happens to be non-empty at query time; callers that query
        // repeatedly without further updates in between are unaffected after the first call
        // pays that cost, since nothing here persists the flushed clone.
        if self.count == 0 {
            return f64::NAN;
        }
        let q = q.clamp(0.0, 1.0);

        let mut flushed = self.clone();
        flushed.flush();

        if flushed.centroids.is_empty() {
            // All samples collapsed into a single point (or count > 0 but somehow no
            // centroids survived, which should not happen) — min/max/either is the best answer.
            return self.min;
        }
        if q <= 0.0 {
            return self.min;
        }
        if q >= 1.0 {
            return self.max;
        }

        let target_weight = q * flushed.count as f64;
        let mut cumulative = 0.0f64;

        for window in flushed.centroids.windows(2) {
            let (left, right) = (&window[0], &window[1]);
            let left_upper = cumulative + left.weight as f64;
            if target_weight <= left_upper {
                // Interpolate within/around `left`'s span, falling back toward the exact min
                // for the very first centroid.
                let span_start = if cumulative == 0.0 {
                    self.min
                } else {
                    left.mean
                };
                let span_end = left.mean;
                if left_upper == cumulative {
                    return span_end;
                }
                let fraction = (target_weight - cumulative) / (left_upper - cumulative);
                return span_start + fraction * (span_end - span_start);
            }
            cumulative = left_upper;
            let _ = right; // considered on the next loop iteration as `left`
        }

        // Fell through: target weight lands in (or past) the last centroid's span.
        let last = flushed.centroids.last().expect("checked non-empty above");
        let last_lower = flushed.count as f64 - last.weight as f64;
        if target_weight <= last_lower {
            return last.mean;
        }
        let fraction = (target_weight - last_lower) / (flushed.count as f64 - last_lower).max(1.0);
        last.mean + fraction * (self.max - last.mean)
    }
}

impl Default for TDigest {
    fn default() -> Self {
        Self::identity()
    }
}

impl PartialEq for TDigest {
    /// Structural equality, provided mainly so `#[derive(PartialEq)]`-style test helpers in
    /// other aggregates keep working uniformly. Because merges are only *approximately*
    /// associative/commutative (see the module docs), two digests built from the same samples
    /// via different merge orders are **not** guaranteed to compare equal here — tests that
    /// check approximate-associativity should compare `quantile()` outputs within a tolerance
    /// instead of relying on this impl.
    fn eq(&self, other: &Self) -> bool {
        self.count == other.count
            && self.min == other.min
            && self.max == other.max
            && self.compression == other.compression
            && self.buffer == other.buffer
            && self.centroids == other.centroids
    }
}

impl Monoid for TDigest {
    fn identity() -> Self {
        TDigest {
            centroids: Vec::new(),
            buffer: Vec::new(),
            compression: DEFAULT_TDIGEST_COMPRESSION,
            count: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }

    fn merge(&self, other: &Self) -> Self {
        let mut result = self.clone();
        result.merge_in_place(other);
        result
    }

    fn merge_in_place(&mut self, other: &Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = other.clone();
            return;
        }

        let target = self.compression.min(other.compression);
        self.recompress_to(target);

        let mut rhs = other.clone();
        rhs.recompress_to(target);

        let mut all: Vec<Centroid> = self.centroids.drain(..).chain(rhs.centroids).collect();
        all.sort_by(|a, b| a.mean.total_cmp(&b.mean));

        self.count += rhs.count;
        self.min = self.min.min(rhs.min);
        self.max = self.max.max(rhs.max);
        self.compression = target;
        self.centroids = Self::compress_sorted(all, target, self.count);
        // Buffers were already flushed by `recompress_to` above, so no buffered samples from
        // either side are lost.
    }
}

impl Aggregator for TDigest {
    type Input = f64;

    const NAME: &'static str = "tdigest";

    fn update(&self, value: f64) -> Self {
        let mut result = self.clone();
        result.update_in_place(value);
        result
    }

    fn update_in_place(&mut self, value: f64) {
        self.count += 1;
        self.min = self.min.min(value);
        self.max = self.max.max(value);
        self.buffer.push(value);

        let capacity = self.compression.max(1) * BUFFER_CAPACITY_MULTIPLIER;
        if self.buffer.len() >= capacity {
            self.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn identity_has_no_samples() {
        let d = TDigest::identity();
        assert_eq!(d.count, 0);
        assert!(d.quantile(0.5).is_nan());
    }

    #[test]
    fn update_tracks_min_max_count() {
        let d = TDigest::identity().update(3.0).update(1.0).update(2.0);
        assert_eq!(d.count, 3);
        assert_eq!(d.min, 1.0);
        assert_eq!(d.max, 3.0);
    }

    #[test]
    fn quantile_zero_and_one_are_exact_min_max() {
        let d = TDigest::identity()
            .update(5.0)
            .update(1.0)
            .update(9.0)
            .update(3.0);
        assert_eq!(d.quantile(0.0), 1.0);
        assert_eq!(d.quantile(1.0), 9.0);
    }

    #[test]
    fn quantile_median_is_reasonable_on_uniform_data() {
        let mut d = TDigest::identity();
        for i in 0..1000 {
            d.update_in_place(i as f64);
        }
        let median = d.quantile(0.5);
        assert!(
            approx_eq(median, 499.5, 20.0),
            "median estimate {median} too far from true median 499.5"
        );
    }

    #[test]
    fn merge_with_identity_is_noop_on_quantiles() {
        let d = TDigest::identity().update(1.0).update(2.0).update(3.0);
        let merged = d.merge(&TDigest::identity());
        assert_eq!(merged.count, d.count);
        assert_eq!(merged.quantile(0.5), d.quantile(0.5));
    }

    #[test]
    fn merge_combines_counts_and_extremes() {
        let a = TDigest::identity().update(1.0).update(2.0);
        let b = TDigest::identity().update(10.0).update(20.0);
        let merged = a.merge(&b);
        assert_eq!(merged.count, 4);
        assert_eq!(merged.min, 1.0);
        assert_eq!(merged.max, 20.0);
    }

    #[test]
    fn merge_recompresses_to_the_smaller_compression() {
        let mut a = TDigest::identity();
        a.compression = 200;
        for i in 0..500 {
            a.update_in_place(i as f64);
        }

        let mut b = TDigest::identity();
        b.compression = 20;
        for i in 0..500 {
            b.update_in_place((i + 500) as f64);
        }

        let merged = a.merge(&b);
        assert_eq!(merged.compression, 20);
        assert!(merged.centroids.len() <= 20 + 4); // small slack for greedy-merge boundary effects
    }

    #[test]
    fn merge_is_approximately_associative() {
        let a = TDigest::identity().update(1.0).update(2.0).update(3.0);
        let b = TDigest::identity().update(4.0).update(5.0);
        let c = TDigest::identity().update(6.0).update(7.0).update(8.0);

        let left = a.merge(&b).merge(&c);
        let right = a.merge(&b.merge(&c));

        assert!(approx_eq(left.quantile(0.5), right.quantile(0.5), 0.5));
        assert_eq!(left.count, right.count);
    }

    #[test]
    fn buffer_flushes_past_capacity() {
        let mut d = TDigest::identity();
        d.compression = 10;
        let capacity = d.compression * BUFFER_CAPACITY_MULTIPLIER;
        for i in 0..capacity {
            d.update_in_place(i as f64);
        }
        // One more update should trigger a flush given the buffer was already at capacity.
        d.update_in_place(capacity as f64);
        assert!(d.buffer.len() < capacity);
    }
}
