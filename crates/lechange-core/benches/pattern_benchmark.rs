use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lechange_core::interner::StringInterner;
use lechange_core::patterns::matcher::PatternMatcher;
use lechange_core::types::ChangedFile;

fn generate_test_paths(count: usize) -> Vec<String> {
    let extensions = ["rs", "py", "js", "ts", "go", "java", "cpp", "c", "h", "md"];
    let directories = [
        "src", "tests", "benches", "examples", "docs", "lib", "api", "core",
    ];

    (0..count)
        .map(|i| {
            let ext = extensions[i % extensions.len()];
            let dir = directories[i % directories.len()];
            format!("{}/subdir/file_{}.{}", dir, i, ext)
        })
        .collect()
}

fn bench_pattern_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("pattern_compilation");

    let pattern_sets = vec![
        (vec!["**/*.rs"], "single_extension"),
        (
            vec!["**/*.rs", "**/*.toml", "**/*.md"],
            "multiple_extensions",
        ),
        (
            vec!["src/**", "tests/**", "benches/**"],
            "directory_patterns",
        ),
        (vec!["**/*.{rs,toml,md}"], "brace_expansion"),
        (
            vec![
                "src/**/*.rs",
                "tests/**/*.rs",
                "benches/**/*.rs",
                "examples/**/*.rs",
            ],
            "complex_patterns",
        ),
    ];

    for (patterns, name) in pattern_sets {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &patterns,
            |b, patterns| {
                let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_ref()).collect();
                b.iter(|| PatternMatcher::new(black_box(&pattern_refs), &[], false));
            },
        );
    }

    group.finish();
}

fn bench_single_path_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_path_matching");

    let patterns = ["**/*.rs", "**/*.toml", "**/*.md"];
    let matcher = PatternMatcher::new(&patterns, &[], false).unwrap();

    let test_paths = vec![
        ("match_simple", "src/main.rs"),
        ("match_nested", "src/deep/nested/path/file.rs"),
        ("no_match", "src/main.py"),
        ("match_toml", "Cargo.toml"),
        ("match_docs", "docs/README.md"),
    ];

    for (name, path) in test_paths {
        group.bench_with_input(BenchmarkId::from_parameter(name), &path, |b, &path| {
            b.iter(|| matcher.matches_sync(black_box(path)));
        });
    }

    group.finish();
}

fn bench_bulk_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("bulk_filtering");

    let patterns = ["**/*.rs", "**/*.toml"];
    let matcher = PatternMatcher::new(&patterns, &[], false).unwrap();

    for count in [10, 100, 1000, 10000] {
        let interner = StringInterner::with_capacity(count);
        let paths = generate_test_paths(count);

        let files: Vec<ChangedFile> = paths
            .iter()
            .map(|path| ChangedFile {
                path: interner.intern(path),
                previous_path: None,
                change_type: lechange_core::types::ChangeType::Modified,
                is_symlink: false,
                submodule_depth: 0,
                origin: Default::default(),
            })
            .collect();

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &files, |b, files| {
            b.iter(|| matcher.partition_files_parallel(black_box(files), &interner));
        });
    }

    group.finish();
}

fn bench_sequential_vs_parallel(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_vs_parallel");

    let patterns = ["**/*.rs"];
    let matcher = PatternMatcher::new(&patterns, &[], false).unwrap();

    for count in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(count);
        let paths = generate_test_paths(count);

        let files: Vec<ChangedFile> = paths
            .iter()
            .map(|path| ChangedFile {
                path: interner.intern(path),
                previous_path: None,
                change_type: lechange_core::types::ChangeType::Modified,
                is_symlink: false,
                submodule_depth: 0,
                origin: Default::default(),
            })
            .collect();

        group.throughput(Throughput::Elements(count as u64));

        // Sequential benchmark
        group.bench_with_input(BenchmarkId::new("sequential", count), &files, |b, files| {
            b.iter(|| {
                files
                    .iter()
                    .filter(|file| {
                        interner
                            .resolve(file.path)
                            .map(|path| matcher.matches_sync(path))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            });
        });

        // Parallel benchmark
        group.bench_with_input(BenchmarkId::new("parallel", count), &files, |b, files| {
            b.iter(|| matcher.partition_files_parallel(black_box(files), &interner));
        });
    }

    group.finish();
}

fn bench_negation_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("negation_patterns");

    let include_patterns = ["**/*"];
    let exclude_patterns = ["**/node_modules/**", "**/target/**", "**/.git/**"];

    let matcher = PatternMatcher::new(&include_patterns, &exclude_patterns, false).unwrap();

    let test_paths = vec![
        "src/main.rs",                   // Should match
        "node_modules/package/index.js", // Should not match
        "target/debug/app",              // Should not match
        ".git/config",                   // Should not match
        "tests/test.rs",                 // Should match
    ];

    for path in test_paths {
        group.bench_with_input(BenchmarkId::from_parameter(path), &path, |b, &path| {
            b.iter(|| matcher.matches_sync(black_box(path)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_pattern_compilation,
    bench_single_path_matching,
    bench_bulk_filtering,
    bench_sequential_vs_parallel,
    bench_negation_patterns
);
criterion_main!(benches);
