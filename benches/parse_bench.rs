//! Benchmarks comparing full re-parse vs incremental parse.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use cinccino::incremental::{IncrementalParser, TextEdit};
use cinccino::parser;

/// Build a synthetic multi-template source by repeating a template body.
fn make_large_source(num_templates: usize) -> String {
    let mut src = String::from("pragma circom \"2.2.3\";\n\n");
    for i in 0..num_templates {
        src.push_str(&format!(
            "template T{i}(n) {{\n\
             \tsignal input inp;\n\
             \tsignal output out;\n\
             \tvar acc = 0;\n\
             \tfor (var j = 0; j < n; j++) {{\n\
             \t\tacc += inp * j;\n\
             \t}}\n\
             \tout <== acc;\n\
             }}\n\n"
        ));
    }
    src
}

/// Replace `acc` with `total` inside a single template body, simulating a
/// typical single-file edit.
fn make_edit(old_src: &str, target_template: usize) -> TextEdit {
    // Find the nth occurrence of "var acc" in old_src.
    let mut offset = 0;
    for _ in 0..target_template {
        offset = old_src[offset..].find("var acc").unwrap() + offset + 1;
    }
    let edit_start = old_src[offset..].find("var acc").unwrap() + offset + 4; // after "var "
    let old_word = "acc";
    let new_word = "total";
    // Verify
    assert_eq!(&old_src[edit_start..edit_start + old_word.len()], old_word);
    TextEdit {
        start: edit_start,
        removed: old_word.len(),
        inserted: new_word.len(),
    }
}

fn apply_rename(src: &str, target_template: usize) -> String {
    // Replace "acc" with "total" in the target template only.
    let mut result = String::new();
    let mut template_count = 0;
    let mut in_target = false;
    let mut i = 0;
    let bytes = src.as_bytes();
    while i < bytes.len() {
        if i + 8 < bytes.len() && &src[i..i + 8] == "template" {
            in_target = template_count == target_template;
            template_count += 1;
        }
        if in_target && i + 3 <= bytes.len() && &src[i..i + 3] == "acc" {
            // Check it's not part of a larger word.
            let before_ok = i == 0 || !src.as_bytes()[i - 1].is_ascii_alphanumeric();
            let after_ok = i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric();
            if before_ok && after_ok {
                result.push_str("total");
                i += 3;
                continue;
            }
        }
        result.push(src.as_bytes()[i] as char);
        i += 1;
    }
    result
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for num_templates in [10, 50, 100] {
        let old_src = make_large_source(num_templates);
        // Edit the template in the middle.
        let target = num_templates / 2;
        let new_src = apply_rename(&old_src, target);
        let edit = make_edit(&old_src, target);

        group.bench_with_input(
            BenchmarkId::new("full_reparse", num_templates),
            &new_src,
            |b, src| {
                b.iter(|| {
                    let (_file, _errors) = parser::parse(src);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("incremental", num_templates),
            &(&old_src, &new_src, &edit),
            |b, &(old, new, ed)| {
                b.iter_batched(
                    || IncrementalParser::parse(old),
                    |mut inc| {
                        inc.update(new, ed);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
