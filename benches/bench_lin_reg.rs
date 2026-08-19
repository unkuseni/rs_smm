
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ndarray::array;
use rs_smm::features::linear_reg::mid_price_regression;


fn benchmark_linfa_linear(c: &mut Criterion) {
    // Example data
    let mid_price_array = array![1.0, 2.0, 3.0, 4.0, 5.0];
    let features = array![[1.0, 2.0], [2.0, 3.0], [3.0, 4.0], [4.0, 5.0], [5.0, 6.0]];
    let curr_spread = 1.0;

    c.bench_function("linfa_linear_regression", |b| {
        b.iter(|| {
        black_box(mid_price_regression(mid_price_array.clone(), features.clone(), curr_spread))
        })
    });
}

criterion_group!(benches, benchmark_linfa_linear);
criterion_main!(benches);
