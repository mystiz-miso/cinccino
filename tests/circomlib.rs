//! Integration tests: parse real circomlib standard library files.

use cinccino::parser::parse;

fn parse_fixture(filename: &str) {
    let path = format!("{}/tests/fixtures/{}", env!("CARGO_MANIFEST_DIR"), filename);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e));
    let (file, errors) = parse(&source);
    assert!(
        errors.is_empty(),
        "parse errors in {}:\n{:#?}",
        filename,
        errors
    );
    assert!(
        !file.items.is_empty(),
        "{} should produce at least one item",
        filename
    );
}

#[test]
fn test_parse_bitify() {
    parse_fixture("bitify.circom");
}

#[test]
fn test_parse_comparators() {
    parse_fixture("comparators.circom");
}

#[test]
fn test_parse_gates() {
    parse_fixture("gates.circom");
}

#[test]
fn test_parse_montgomery() {
    parse_fixture("montgomery.circom");
}

#[test]
fn test_parse_poseidon() {
    parse_fixture("poseidon.circom");
}

#[test]
fn test_parse_babyjub() {
    parse_fixture("babyjub.circom");
}

#[test]
fn test_parse_multiplexer() {
    parse_fixture("multiplexer.circom");
}

#[test]
fn test_parse_escalarmulany() {
    parse_fixture("escalarmulany.circom");
}
