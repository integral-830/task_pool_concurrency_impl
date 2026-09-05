use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use task_pool::foundations::row_vs_col::{ColStore, ColumnTable, PaxStore, RowStore};

const ROWS: usize = 1_000_000;
const COLUMN: usize = 5;
const ROW: usize = 500_000;
const PAGE_ROWS: usize = 128;

fn populate<T: ColumnTable>() -> T {
    let mut table = T::new();

    for i in 0..ROWS {
        table.insert([
            i as i64,
            i as i64 + 1,
            i as i64 + 2,
            i as i64 + 3,
            i as i64 + 4,
            i as i64 + 5,
            i as i64 + 6,
            i as i64 + 7,
            i as i64 + 8,
            i as i64 + 9,
        ]);
    }

    table
}

fn bench_sum_column(c: &mut Criterion) {
    let mut row = populate::<RowStore>();
    let mut pax = populate::<PaxStore<PAGE_ROWS>>();
    let mut col = populate::<ColStore>();

    let mut group = c.benchmark_group("sum_column");

    group.bench_function(BenchmarkId::new("row", ROWS), |b| {
        b.iter(|| black_box(row.get_col_sum(black_box(COLUMN))));
    });

    group.bench_function(BenchmarkId::new("pax", ROWS), |b| {
        b.iter(|| black_box(pax.get_col_sum(black_box(COLUMN))));
    });

    group.bench_function(BenchmarkId::new("column", ROWS), |b| {
        b.iter(|| black_box(col.get_col_sum(black_box(COLUMN))));
    });

    group.finish();
}

fn bench_get_row(c: &mut Criterion) {
    let row = populate::<RowStore>();
    let pax = populate::<PaxStore<PAGE_ROWS>>();
    let col = populate::<ColStore>();

    let mut group = c.benchmark_group("get_row");

    group.bench_function(BenchmarkId::new("row", ROWS), |b| {
        b.iter(|| {
            let mut result = [0i64; 10];

            for column in 0..10 {
                result[column] = row.get(black_box(ROW), black_box(column));
            }

            black_box(result);
        });
    });

    group.bench_function(BenchmarkId::new("pax", ROWS), |b| {
        b.iter(|| {
            let mut result = [0i64; 10];

            for column in 0..10 {
                result[column] = pax.get(black_box(ROW), black_box(column));
            }

            black_box(result);
        });
    });

    group.bench_function(BenchmarkId::new("column", ROWS), |b| {
        b.iter(|| {
            let mut result = [0i64; 10];

            for column in 0..10 {
                result[column] = col.get(black_box(ROW), black_box(column));
            }

            black_box(result);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_sum_column, bench_get_row);
criterion_main!(benches);
