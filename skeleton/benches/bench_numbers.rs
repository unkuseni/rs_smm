use criterion::{black_box, criterion_group, criterion_main, Criterion};
use skeleton::util::helpers::{
    calculate_exponent, generate_timestamp, geometric_weights, geomspace, linspace, nbsqrt,
    round_step, spread_price_in_bps, Round,
};

fn bench_geomspace(c: &mut Criterion) {
    c.bench_function("geomspace", |b| {
        b.iter(|| {
            black_box(geomspace(0.0005, 1.0, 10));
        })
    });
}

fn bench_geometric_weights(c: &mut Criterion) {
    c.bench_function("geometric_weights", |b| {
        b.iter(|| {
            black_box(geometric_weights(0.05, 30, false));
        })
    });
}

fn bench_places(c: &mut Criterion) {
    c.bench_function("places", |b| {
        b.iter(|| {
            black_box(0.0000567.count_decimal_places());
        })
    });
}

fn bench_time(c: &mut Criterion) {
    c.bench_function("time", |b| {
        b.iter(|| {
            black_box(generate_timestamp());
        })
    });
}

fn bench_round(c: &mut Criterion) {
    c.bench_function("round", |b| {
        b.iter(|| {
            black_box(0.0004053456.round_to(5));
        })
    });
}

fn bench_round_step(c: &mut Criterion) {
    c.bench_function("round_step", |b| {
        b.iter(|| {
            black_box(round_step(0.04053456, 0.00002));
        })
    });
}

fn bench_exp(c: &mut Criterion) {
    c.bench_function("exp", |b| {
        b.iter(|| {
            black_box(calculate_exponent(5.0));
        })
    });
}

fn bench_linspace(c: &mut Criterion) {
    c.bench_function("linspace", |b| {
        b.iter(|| {
            black_box(linspace(0.65, 1.2054, 25));
        })
    });
}

fn bench_sqrt(c: &mut Criterion) {
    c.bench_function("sqrt", |b| {
        b.iter(|| {
            black_box(nbsqrt(5.0));
        })
    });
}

fn bench_spread_bps(c: &mut Criterion) {
    c.bench_function("spread_bps", |b| {
        b.iter(|| {
            black_box(spread_price_in_bps(0.0003, 0.4053));
        })
    });
}

fn bench_clip(c: &mut Criterion) {
    c.bench_function("clip", |b| {
        b.iter(|| {
            black_box(0.0000567.clip(0.00001, 0.0001));
        })
    });
}

criterion_group!(
    benches,
    bench_geomspace,
    bench_geometric_weights,
    bench_places,
    bench_time,
    bench_round,
    bench_round_step,
    bench_exp,
    bench_linspace,
    bench_sqrt,
    bench_spread_bps,
    bench_clip
);
criterion_main!(benches);
