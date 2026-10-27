//! `jokerd::shared::Histogram`: the start jitter and tick duration
//! percentiles of `GET /v1/metrics` (docs/01 §19, ref/09 §5.2).

use jokerd::shared::Histogram;

#[test]
fn every_value_lands_in_a_bucket_at_most_a_quarter_wide() {
    let mut v = 0u64;
    while v < 1 << 62 {
        let upper = Histogram::upper(Histogram::bucket(v));
        assert!(upper >= v, "{v} above its bucket's {upper}");
        assert!(upper <= v + v / 4, "{v}: bucket up to {upper}");
        v = v * 3 / 2 + 1;
    }
    assert_eq!(Histogram::upper(Histogram::bucket(u64::MAX)), u64::MAX);
}

#[test]
fn quantiles_come_from_the_recorded_values() {
    let h = Histogram::default();
    assert_eq!(h.quantile(0.5), None);
    assert_eq!(h.first_mono_ns(), None);
    for v in 1..=1000u64 {
        h.record(v * 1_000, 7 + v);
    }
    assert_eq!(h.first_mono_ns(), Some(8));
    let p50 = h.quantile(0.5).unwrap();
    assert!((500_000..=625_000).contains(&p50), "p50 {p50}");
    let p99 = h.quantile(0.99).unwrap();
    assert!((990_000..=1_237_500).contains(&p99), "p99 {p99}");
    let p999 = h.quantile(0.999).unwrap();
    assert!(p999 >= 999_000, "p999 {p999}");
    assert!(h.quantile(1.0).unwrap() >= 1_000_000);
}
