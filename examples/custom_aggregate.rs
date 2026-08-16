//! Demonstrates adding a brand new aggregate — running **variance** — entirely from outside the
//! crate's core modules, to prove the claim that new aggregates never require touching
//! `engine`, `bucket`, or `aggregate_set`.
//!
//! The state uses [Welford's online algorithm] for numerically stable running variance, plus its
//! parallel/merge variant (Chan et al.) so it can be combined with another partial state without
//! looking at any raw samples again — exactly the property [`Monoid`] requires.
//!
//! [Welford's online algorithm]: https://en.wikipedia.org/wiki/Algorithms_for_calculating_variance#Welford's_online_algorithm
//!
//! Run with:
//! ```text
//! cargo run --example custom_aggregate
//! ```

use accreta::aggregate_set::Schema;
use accreta::aggregates::Count;
use accreta::aggregator::Aggregator;
use accreta::bucket::BucketLevel;
use accreta::engine::Engine;
use accreta::measures::{FromValue, MeasureNumber};
use accreta::monoid::Monoid;
use chrono::{Duration, TimeZone, Utc};

/// Running variance, tracked as `(n, mean, m2)` via Welford's algorithm.
///
/// `m2` is the running sum of squared differences from the mean; population variance is
/// `m2 / n` and sample variance is `m2 / (n - 1)`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Variance<T> {
    n: u64,
    mean: f64,
    m2: f64,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Variance<T>
where
    T: MeasureNumber + Copy + Default + Into<f64>,
{
    /// Population variance, or `None` if fewer than one sample has been seen.
    fn population(&self) -> Option<f64> {
        (self.n > 0).then_some(self.m2 / self.n as f64)
    }

    /// Sample variance (Bessel-corrected), or `None` if fewer than two samples have been seen.
    fn sample(&self) -> Option<f64> {
        (self.n > 1).then_some(self.m2 / (self.n - 1) as f64)
    }

    /// Sample standard deviation, or `None` if fewer than two samples have been seen.
    fn stddev(&self) -> Option<f64> {
        self.sample().map(f64::sqrt)
    }
}

impl<T> Monoid for Variance<T>
where
    T: MeasureNumber + FromValue,
{
    /// Returns the identity state: zero samples seen. [`Self::population`] and [`Self::sample`]
    /// both return `None` until at least one sample has been folded in.
    fn identity() -> Self {
        Variance {
            n: u64::default(),
            mean: f64::default(),
            m2: f64::default(),
            _marker: std::marker::PhantomData::default(),
        }
    }

    /// Chan et al.'s parallel-variance formula: combine two Welford accumulators without
    /// revisiting either one's underlying samples.
    fn merge(&self, other: &Self) -> Self {
        if self.n == 0 {
            return *other;
        }
        if other.n == 0 {
            return *self;
        }
        let n = self.n + other.n;
        let delta = other.mean - self.mean;
        let mean = self.mean + delta * (other.n as f64 / n as f64);
        let m2 = self.m2 + other.m2 + delta * delta * (self.n as f64 * other.n as f64 / n as f64);
        Variance {
            n,
            mean,
            m2,
            _marker: std::marker::PhantomData::default(),
        }
    }
}

impl<T> Aggregator for Variance<T>
where
    T: MeasureNumber + FromValue + Copy + Default + Into<f64>,
{
    const NAME: &'static str = "variance";

    type Input = T;

    /// One step of Welford's online algorithm.
    fn update(&self, sample: T) -> Self {
        let x: f64 = sample.into();
        let n = self.n + 1;
        let delta = x - self.mean;
        let mean = self.mean + delta / n as f64;
        let delta2 = x - mean;
        let m2 = self.m2 + delta * delta2;
        Variance {
            n,
            mean,
            m2,
            _marker: std::marker::PhantomData::default(),
        }
    }
}

fn main() {
    // Register the new aggregate exactly like a built-in one — Engine, Bucket, and
    // AggregateSet's code is completely unaware that `Variance` didn't ship with the crate.
    let mut builder = Schema::builder();
    builder
        .dimension("browser")
        .measure("visits")
        .with_any::<Count>()
        .with::<Variance<f64>>();
    let schema = builder.build().unwrap();

    let mut engine = Engine::new(schema.clone());

    let start = Utc.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap();
    // Two disjoint batches, ingested and rolled up separately, to show the merge path (not just
    // the single-threaded update path) produces the same numerically stable result.
    let batch_a = [2.0, 4.0, 4.0, 4.0];
    let batch_b = [5.0, 5.0, 7.0, 9.0];

    for (i, v) in batch_a.iter().enumerate() {
        _ = engine.ingest(
            start + Duration::minutes(i as i64),
            vec![*v],
            vec!["Safari"],
        );
    }
    for (i, v) in batch_b.iter().enumerate() {
        _ = engine.ingest(
            start + Duration::minutes(10 + i as i64),
            vec![*v],
            vec!["Safari"],
        );
    }
    engine.rollup();

    let hour_start = BucketLevel::Hour.truncate(start);
    let hour = engine.bucket(BucketLevel::Hour, hour_start).unwrap();
    let aggs = hour.groups().next().unwrap().1;
    for agg in aggs {
        let variance = agg.get::<Variance<f64>>().unwrap();

        println!("n              = {}", agg.get::<Count>().unwrap().value());
        println!("mean           = {:.4}", variance.mean);
        println!("population var = {:.4}", variance.population().unwrap());
        println!("sample var     = {:.4}", variance.sample().unwrap());
        println!("sample stddev  = {:.4}", variance.stddev().unwrap());

        // Known result for [2, 4, 4, 4, 5, 5, 7, 9]: mean 5.0, population variance 4.0.
        assert!((variance.population().unwrap() - 4.0).abs() < 1e-9);
        assert!((variance.mean - 5.0).abs() < 1e-9);
        println!("\n(matches the textbook Welford's-algorithm worked example)");
    }
}
