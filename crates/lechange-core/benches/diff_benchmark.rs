use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use lechange_core::git::diff::DiffParser;
use lechange_core::interner::StringInterner;
use lechange_core::types::ChangeType;

fn generate_diff_output(num_files: usize) -> String {
    let mut output = String::new();

    for i in 0..num_files {
        let change_type = match i % 8 {
            0 => "A",
            1 => "M",
            2 => "D",
            3 => "R100",
            4 => "C100",
            5 => "T",
            6 => "U",
            _ => "X",
        };

        if change_type.starts_with('R') || change_type.starts_with('C') {
            output.push_str(&format!("{}\told/path_{}.rs\tnew/path_{}.rs\n", change_type, i, i));
        } else {
            output.push_str(&format!("{}\tpath/to/file_{}.rs\n", change_type, i));
        }
    }

    output
}

fn bench_diff_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_parsing");

    for size in [10, 100, 1000, 10000] {
        let diff_output = generate_diff_output(size);
        let interner = StringInterner::with_capacity(size * 2);

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &diff_output,
            |b, diff| {
                b.iter(|| {
                    let parser = DiffParser::new(&interner);
                    let lines: Vec<&[u8]> = diff.as_bytes().split(|&b| b == b'\n').collect();

                    for line in lines {
                        if !line.is_empty() {
                            let _ = parser.parse_diff_line(black_box(line));
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_diff_line_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_line_types");
    let interner = StringInterner::new();

    let test_cases = vec![
        ("added", b"A\tsrc/main.rs\n"),
        ("modified", b"M\tsrc/lib.rs\n"),
        ("deleted", b"D\tsrc/old.rs\n"),
        ("renamed", b"R100\told/path.rs\tnew/path.rs\n"),
        ("copied", b"C100\tsrc/original.rs\tsrc/copy.rs\n"),
        ("type_changed", b"T\tsrc/file.rs\n"),
    ];

    for (name, line) in test_cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &line,
            |b, &line| {
                let parser = DiffParser::new(&interner);
                b.iter(|| {
                    let _ = parser.parse_diff_line(black_box(line));
                });
            },
        );
    }

    group.finish();
}

fn bench_change_type_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("change_type_parsing");

    let change_bytes = vec![
        (b'A', "added"),
        (b'M', "modified"),
        (b'D', "deleted"),
        (b'R', "renamed"),
        (b'C', "copied"),
        (b'T', "type_changed"),
        (b'U', "unmerged"),
        (b'X', "unknown"),
    ];

    for (byte, name) in change_bytes {
        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &byte,
            |b, &byte| {
                b.iter(|| {
                    ChangeType::from_byte(black_box(byte))
                });
            },
        );
    }

    group.finish();
}

fn bench_memchr_vs_find(c: &mut Criterion) {
    let mut group = c.benchmark_group("tab_search");

    let test_line = b"M\tpath/to/very/long/file/name/that/simulates/real/repository/structure.rs";

    group.bench_function("memchr", |b| {
        b.iter(|| {
            memchr::memchr(b'\t', black_box(test_line))
        });
    });

    group.bench_function("std_find", |b| {
        b.iter(|| {
            let s = std::str::from_utf8(test_line).unwrap();
            s.find('\t')
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_diff_parsing,
    bench_diff_line_types,
    bench_change_type_parsing,
    bench_memchr_vs_find
);
criterion_main!(benches);
