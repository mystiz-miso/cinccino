//! Integration test: parse every file in circomlib without errors.

use cinccino::parser::parse;
use std::fs;

#[test]
fn test_parse_all_circomlib_files() {
    let fixtures_dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut total = 0;
    let mut failed = Vec::new();

    for entry in fs::read_dir(&fixtures_dir).expect("fixtures dir") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "circom") {
            continue;
        }

        // Skip auto-generated constant tables (25K+ lines, only contain
        // function defs with large array literals — no new syntax).
        let name_str = path.file_name().unwrap().to_string_lossy();
        const SKIP_FILES: &[&str] = &[
            "poseidon_constants.circom",
            "poseidon_constants_old.circom",
            "mimcsponge_constants.circom",
        ];
        if SKIP_FILES.iter().any(|&f| name_str == f) {
            continue;
        }

        total += 1;
        let source = fs::read_to_string(&path).unwrap();
        let (_file, errors) = parse(&source);
        if !errors.is_empty() {
            failed.push((
                path.file_name().unwrap().to_string_lossy().into_owned(),
                errors,
            ));
        }
    }

    if !failed.is_empty() {
        let mut msg = format!(
            "{}/{} circomlib files had parse errors:\n",
            failed.len(),
            total
        );
        for (name, errors) in &failed {
            msg.push_str(&format!("\n  {}:\n", name));
            for e in errors {
                msg.push_str(&format!(
                    "    [{}-{}] {}\n",
                    e.span.start, e.span.end, e.message
                ));
            }
        }
        panic!("{}", msg);
    }

    eprintln!("Successfully parsed {total} circomlib files");
}
