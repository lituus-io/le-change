use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lechange_core::{
    interner::StringInterner,
    types::{ChangedFile, ChangeType, DiffResult},
    patterns::matcher::PatternMatcher,
};
use std::borrow::Cow;

fn generate_realistic_diff_result(num_files: usize, interner: &StringInterner) -> DiffResult {
    let mut files = Vec::with_capacity(num_files);

    for i in 0..num_files {
        let change_type = match i % 5 {
            0 => ChangeType::Added,
            1 => ChangeType::Modified,
            2 => ChangeType::Deleted,
            3 => ChangeType::Renamed,
            _ => ChangeType::Copied,
        };

        let path = format!("src/module_{}/file_{}.rs", i / 100, i);
        let previous_path = if matches!(change_type, ChangeType::Renamed | ChangeType::Copied) {
            Some(interner.intern(&format!("old/module_{}/file_{}.rs", i / 100, i)))
        } else {
            None
        };

        files.push(ChangedFile {
            path: interner.intern(&path),
            previous_path,
            change_type,
            is_symlink: i % 50 == 0, // 2% symlinks
            submodule_depth: if i % 100 == 0 { 1 } else { 0 }, // 1% in submodules
        });
    }

    DiffResult {
        files,
        base_sha: Cow::Borrowed("abc123"),
        head_sha: Cow::Borrowed("def456"),
    }
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result,
            |b, diff_result| {
                b.iter(|| {
                    // Simulate full processing pipeline
                    let patterns = vec!["**/*.rs", "**/*.toml"];
                    let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
                    let matcher = PatternMatcher::new(&pattern_refs, &[], false).unwrap();

                    // Filter by patterns
                    let filtered = matcher.filter_files_parallel(
                        black_box(&diff_result.files),
                        &interner
                    );

                    // Group by change type
                    let mut added = 0;
                    let mut modified = 0;
                    let mut deleted = 0;

                    for file in &filtered {
                        match file.change_type {
                            ChangeType::Added => added += 1,
                            ChangeType::Modified => modified += 1,
                            ChangeType::Deleted => deleted += 1,
                            _ => {}
                        }
                    }

                    black_box((filtered, added, modified, deleted))
                });
            },
        );
    }

    group.finish();
}

fn bench_grouping_by_change_type(c: &mut Criterion) {
    let mut group = c.benchmark_group("grouping_by_change_type");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    let mut groups = std::collections::HashMap::new();

                    for file in files {
                        groups.entry(file.change_type)
                            .or_insert_with(Vec::new)
                            .push(file);
                    }

                    black_box(groups)
                });
            },
        );
    }

    group.finish();
}

fn bench_path_resolution(c: &mut Criterion) {
    let mut group = c.benchmark_group("path_resolution");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    let mut paths = Vec::with_capacity(files.len());

                    for file in files {
                        if let Some(path) = interner.resolve(file.path) {
                            paths.push(path);
                        }
                    }

                    black_box(paths)
                });
            },
        );
    }

    group.finish();
}

fn bench_filter_symlinks(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_symlinks");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    let non_symlinks: Vec<_> = files.iter()
                        .filter(|f| !f.is_symlink)
                        .cloned()
                        .collect();

                    black_box(non_symlinks)
                });
            },
        );
    }

    group.finish();
}

fn bench_filter_submodules(c: &mut Criterion) {
    let mut group = c.benchmark_group("filter_submodules");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    let root_files: Vec<_> = files.iter()
                        .filter(|f| f.submodule_depth == 0)
                        .cloned()
                        .collect();

                    black_box(root_files)
                });
            },
        );
    }

    group.finish();
}

fn bench_count_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("count_operations");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    let added = files.iter().filter(|f| f.change_type == ChangeType::Added).count();
                    let modified = files.iter().filter(|f| f.change_type == ChangeType::Modified).count();
                    let deleted = files.iter().filter(|f| f.change_type == ChangeType::Deleted).count();
                    let renamed = files.iter().filter(|f| f.change_type == ChangeType::Renamed).count();

                    black_box((added, modified, deleted, renamed))
                });
            },
        );
    }

    group.finish();
}

fn bench_parallel_pattern_filtering(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_pattern_filtering");

    let patterns = vec!["**/*.rs", "**/*.toml", "**/*.md"];
    let pattern_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();
    let matcher = PatternMatcher::new(&pattern_refs, &[], false).unwrap();

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_result.files,
            |b, files| {
                b.iter(|| {
                    matcher.filter_files_parallel(black_box(files), &interner)
                });
            },
        );
    }

    group.finish();
}

fn bench_json_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_serialization");

    for size in [100, 1000, 10000] {
        let interner = StringInterner::with_capacity(size * 2);
        let diff_result = generate_realistic_diff_result(size, &interner);

        // Convert to strings
        let paths: Vec<String> = diff_result.files
            .iter()
            .filter_map(|f| interner.resolve(f.path).map(|s| s.to_string()))
            .collect();

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &paths,
            |b, paths| {
                b.iter(|| {
                    serde_json::to_string(black_box(paths)).unwrap()
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_full_pipeline,
    bench_grouping_by_change_type,
    bench_path_resolution,
    bench_filter_symlinks,
    bench_filter_submodules,
    bench_count_operations,
    bench_parallel_pattern_filtering,
    bench_json_serialization
);
criterion_main!(benches);
