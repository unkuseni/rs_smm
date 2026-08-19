
use bybit::model::{Ask, Bid};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use skeleton::util::localorderbook::LocalBook;


fn bench_local_book(c: &mut Criterion) {
    c.bench_function("local_book", |b| {
        let mut book = LocalBook::new();
        b.iter(|| {
            book.update_bba(
                black_box(vec![
                    Bid {
                        price: 100.0,
                        qty: 1.0,
                    },
                    Bid {
                        price: 99.0,
                        qty: 1.0,
                    },
                    Bid {
                        price: 98.0,
                        qty: 1.0,
                    },
                    Bid {
                        price: 97.0,
                        qty: 1.0,
                    },
                    Bid {
                        price: 96.0,
                        qty: 1.0,
                    },
                ]),
                black_box(vec![
                    Ask {
                        price: 101.0,
                        qty: 1.0,
                    },
                    Ask {
                        price: 100.0,
                        qty: 1.0,
                    },
                    Ask {
                        price: 99.0,
                        qty: 1.0,
                    },
                    Ask {
                        price: 98.0,
                        qty: 1.0,
                    },
                    Ask {
                        price: 97.0,
                        qty: 1.0,
                    },
                ]),
                black_box(10),
            );
        })
    });
}

criterion_group!(benches, bench_local_book);
criterion_main!(benches);
