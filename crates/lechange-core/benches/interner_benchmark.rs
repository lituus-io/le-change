use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lechange_core::interner::StringInterner;
use std::sync::Arc;
use std::thread;

fn generate_test_strings(count: usize, unique: bool) -> Vec<String> {
    if unique {
        (0..count)
            .map(|i| format!("path/to/file_{}.rs", i))
            .collect()
    } else {
        // Simulate realistic workload with duplicates
        let unique_paths = vec![
            "src/main.rs",
            "src/lib.rs",
            "tests/test.rs",
            "Cargo.toml",
            "README.md",
        ];

        (0..count)
            .map(|i| unique_paths[i % unique_paths.len()].to_string())
            .collect()
    }
}

fn bench_intern_new_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("intern_new_strings");

    for count in [10, 100, 1000, 10000] {
        let strings = generate_test_strings(count, true);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &strings,
            |b, strings| {
                b.iter(|| {
                    let interner = StringInterner::with_capacity(count);
                    for s in strings {
                        let _ = interner.intern(black_box(s));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_intern_duplicate_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("intern_duplicate_strings");

    for count in [10, 100, 1000, 10000] {
        let strings = generate_test_strings(count, false);

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &strings,
            |b, strings| {
                b.iter(|| {
                    let interner = StringInterner::with_capacity(10);
                    for s in strings {
                        let _ = interner.intern(black_box(s));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_resolve_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolve_strings");

    for count in [10, 100, 1000, 10000] {
        let strings = generate_test_strings(count, true);
        let interner = StringInterner::with_capacity(count);

        let ids: Vec<_> = strings.iter().map(|s| interner.intern(s)).collect();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &ids,
            |b, ids| {
                b.iter(|| {
                    for &id in ids {
                        let _ = interner.resolve(black_box(id));
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_intern_cached_string(c: &mut Criterion) {
    let mut group = c.benchmark_group("intern_cached_string");

    let interner = StringInterner::new();
    let test_string = "src/main.rs";

    // Pre-intern the string
    let _ = interner.intern(test_string);

    group.bench_function("cached_intern", |b| {
        b.iter(|| {
            interner.intern(black_box(test_string))
        });
    });

    group.bench_function("new_intern", |b| {
        b.iter(|| {
            let fresh_interner = StringInterner::new();
            fresh_interner.intern(black_box(test_string))
        });
    });

    group.finish();
}

fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");

    // Test memory savings with duplicates
    for dup_count in [10, 100, 1000] {
        let duplicates = vec!["same/path.rs"; dup_count];

        group.bench_with_input(
            BenchmarkId::new("with_interner", dup_count),
            &duplicates,
            |b, duplicates| {
                b.iter(|| {
                    let interner = StringInterner::new();
                    let ids: Vec<_> = duplicates.iter()
                        .map(|&s| interner.intern(black_box(s)))
                        .collect();
                    black_box(ids);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("without_interner", dup_count),
            &duplicates,
            |b, duplicates| {
                b.iter(|| {
                    let strings: Vec<String> = duplicates.iter()
                        .map(|&s| black_box(s).to_string())
                        .collect();
                    black_box(strings);
                });
            },
        );
    }

    group.finish();
}

fn bench_capacity_growth(c: &mut Criterion) {
    let mut group = c.benchmark_group("capacity_growth");

    let strings = generate_test_strings(1000, true);

    group.bench_with_input(
        BenchmarkId::from_parameter("small_initial_capacity"),
        &strings,
        |b, strings| {
            b.iter(|| {
                let interner = StringInterner::with_capacity(10);
                for s in strings {
                    let _ = interner.intern(black_box(s));
                }
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::from_parameter("correct_initial_capacity"),
        &strings,
        |b, strings| {
            b.iter(|| {
                let interner = StringInterner::with_capacity(1000);
                for s in strings {
                    let _ = interner.intern(black_box(s));
                }
            });
        },
    );

    group.finish();
}

fn bench_concurrent_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_access");

    for thread_count in [2, 4, 8] {
        let strings = generate_test_strings(1000, false);
        let interner = Arc::new(StringInterner::with_capacity(100));

        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &thread_count| {
                b.iter(|| {
                    let mut handles = vec![];

                    for _ in 0..thread_count {
                        let interner_clone = Arc::clone(&interner);
                        let strings_clone = strings.clone();

                        let handle = thread::spawn(move || {
                            for s in strings_clone {
                                let _ = interner_clone.intern(&s);
                            }
                        });

                        handles.push(handle);
                    }

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    let strings = generate_test_strings(100, true);
    let interner = StringInterner::with_capacity(100);

    group.bench_function("intern_and_resolve", |b| {
        b.iter(|| {
            for s in &strings {
                let id = interner.intern(black_box(s));
                let _ = interner.resolve(black_box(id));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_intern_new_strings,
    bench_intern_duplicate_strings,
    bench_resolve_strings,
    bench_intern_cached_string,
    bench_memory_efficiency,
    bench_capacity_growth,
    bench_concurrent_access,
    bench_roundtrip
);
criterion_main!(benches);
