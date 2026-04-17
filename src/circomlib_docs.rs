//! Documentation for well-known circomlib templates.
//!
//! When a user hovers over a call to a circomlib template such as
//! `Num2Bits`, the LSP should surface the template's documented purpose,
//! parameters, and output constraints. Rather than parse the upstream
//! circomlib sources (which are not guaranteed to be on disk), we embed a
//! curated static table of markdown docstrings and look up by template name.
//!
//! The docs are deliberately terse — a one-line summary, a `**Params**`
//! bullet list, a `**Signals**` bullet list, and a `**Guarantees**` bullet
//! list for the constraints the template enforces on its outputs. Anyone
//! can extend [`CIRCOMLIB_DOCS`] with further entries without touching the
//! hover provider.

/// A documented circomlib template entry.
#[derive(Debug, Clone, Copy)]
pub struct CircomlibEntry {
    /// Template name (case-sensitive), e.g. `"Num2Bits"`.
    pub name: &'static str,
    /// Markdown docstring surfaced verbatim through the hover provider.
    pub markdown: &'static str,
}

/// Look up docs for a circomlib template by name.
///
/// Returns `None` if the template is not in the embedded table.
pub fn lookup(name: &str) -> Option<&'static CircomlibEntry> {
    CIRCOMLIB_DOCS.iter().find(|e| e.name == name)
}

/// All documented circomlib template names.
///
/// Useful for completion lists and tests.
pub fn known_names() -> impl Iterator<Item = &'static str> {
    CIRCOMLIB_DOCS.iter().map(|e| e.name)
}

/// Curated documentation for common circomlib templates.
///
/// Sources: the circomlib (iden3/circomlib) repository as of 2024-04 and the
/// iden3/circom-docs reference. Only the most commonly imported templates
/// are covered; contributors should extend as needed.
pub const CIRCOMLIB_DOCS: &[CircomlibEntry] = &[
    CircomlibEntry {
        name: "Num2Bits",
        markdown: "**Num2Bits(n)** — decompose a field element into its `n` least-significant bits.

**Params**
- `n`: number of output bits.

**Signals**
- `signal input in`: the field element to decompose.
- `signal output out[n]`: little-endian bit decomposition of `in`.

**Guarantees**
- Each `out[i]` is constrained to be binary (`out[i] * (out[i] - 1) === 0`).
- `Σ out[i] * 2^i === in`, so the decomposition is unique.
- **Underconstrained warning**: for `n ≥ 254` the sum can wrap the field
  modulus, so `out` is *not* uniquely determined by `in`.
",
    },
    CircomlibEntry {
        name: "Num2Bits_strict",
        markdown: "**Num2Bits_strict()** — decompose a field element into 254 bits, asserting the result is below the BN254 prime.

**Signals**
- `signal input in`
- `signal output out[254]`

**Guarantees**
- Same as `Num2Bits(254)`, plus an explicit range check that forbids the wrap-around case.
",
    },
    CircomlibEntry {
        name: "Bits2Num",
        markdown: "**Bits2Num(n)** — recompose a field element from `n` bits.

**Params**
- `n`: number of input bits.

**Signals**
- `signal input in[n]`: bits (caller must ensure each is 0/1).
- `signal output out`: `Σ in[i] * 2^i`.

**Guarantees**
- Linear constraint `out === Σ in[i] * 2^i`.
- **Does NOT** constrain `in[i]` to be binary — pass already-constrained bits.
",
    },
    CircomlibEntry {
        name: "Bits2Num_strict",
        markdown: "**Bits2Num_strict()** — recompose 254 bits into a field element, asserting the value is below the BN254 prime.

**Signals**
- `signal input in[254]`
- `signal output out`
",
    },
    CircomlibEntry {
        name: "IsZero",
        markdown: "**IsZero()** — 1 iff the input equals 0.

**Signals**
- `signal input in`
- `signal output out`: 1 when `in == 0`, else 0.

**Guarantees**
- `out * in === 0` and `out === 1 - in * inv`, where `inv` is a witness.
- Output is always binary.
",
    },
    CircomlibEntry {
        name: "IsEqual",
        markdown: "**IsEqual()** — 1 iff the two inputs are equal.

**Signals**
- `signal input in[2]`
- `signal output out`: 1 when `in[0] == in[1]`, else 0.

**Guarantees**
- Internally computes `IsZero(in[1] - in[0])`.
- Output is always binary.
",
    },
    CircomlibEntry {
        name: "LessThan",
        markdown: "**LessThan(n)** — 1 iff the first input is strictly less than the second.

**Params**
- `n`: the bit width to reason about; **both inputs must be in `[0, 2^n)`** or the result is unsound.

**Signals**
- `signal input in[2]`
- `signal output out`: 1 when `in[0] < in[1]`, else 0.

**Caveats**
- Requires `n <= 252` and range-checked inputs; otherwise the subtraction can wrap.
",
    },
    CircomlibEntry {
        name: "LessEqThan",
        markdown: "**LessEqThan(n)** — 1 iff the first input is `≤` the second (same caveats as `LessThan`).
",
    },
    CircomlibEntry {
        name: "GreaterThan",
        markdown: "**GreaterThan(n)** — 1 iff the first input is strictly greater than the second (same caveats as `LessThan`).
",
    },
    CircomlibEntry {
        name: "GreaterEqThan",
        markdown: "**GreaterEqThan(n)** — 1 iff the first input is `≥` the second (same caveats as `LessThan`).
",
    },
    CircomlibEntry {
        name: "CompConstant",
        markdown: "**CompConstant(ct)** — 1 iff the 254-bit input (as little-endian bits) is strictly greater than the constant `ct`.

**Params**
- `ct`: a compile-time constant representing the threshold.

**Signals**
- `signal input in[254]`: bits (caller should constrain to {0,1}).
- `signal output out`

**Guarantees**
- Output is binary.
",
    },
    CircomlibEntry {
        name: "AliasCheck",
        markdown: "**AliasCheck()** — aborts if a 254-bit decomposition aliases the field (i.e. is `≥` the BN254 prime).

**Signals**
- `signal input in[254]`

Used internally by `Num2Bits_strict`.
",
    },
    CircomlibEntry {
        name: "Poseidon",
        markdown: "**Poseidon(nInputs)** — Poseidon hash (BN254 parameters).

**Params**
- `nInputs`: number of input field elements (1 ≤ nInputs ≤ 16).

**Signals**
- `signal input inputs[nInputs]`
- `signal output out`

Widely used as a SNARK-friendly collision-resistant hash.
",
    },
    CircomlibEntry {
        name: "MiMC7",
        markdown: "**MiMC7(nrounds)** — MiMC block-cipher in a Feistel construction (legacy).

**Params**
- `nrounds`: number of rounds.

**Signals**
- `signal input x_in`
- `signal input k` (key)
- `signal output out`

New code should prefer `Poseidon` — MiMC is kept for backward compatibility.
",
    },
    CircomlibEntry {
        name: "Pedersen",
        markdown: "**Pedersen(n)** — Pedersen hash over the Baby Jubjub curve.

**Params**
- `n`: number of input bits.

**Signals**
- `signal input in[n]` (bits)
- `signal output out[2]` (compressed curve point)
",
    },
    CircomlibEntry {
        name: "EdDSAVerifier",
        markdown: "**EdDSAVerifier(n)** — verify an EdDSA signature over the Baby Jubjub curve.

Use this to authenticate signed messages inside a circuit.
",
    },
    CircomlibEntry {
        name: "Sign",
        markdown: "**Sign()** — extract the sign bit of a field element (1 if the value is in the upper half, else 0).

**Signals**
- `signal input in[254]` (bit-decomposed)
- `signal output sign`
",
    },
    CircomlibEntry {
        name: "Switcher",
        markdown: "**Switcher()** — conditional swap.

**Signals**
- `signal input sel` (must be 0 or 1; caller enforces)
- `signal input L`, `signal input R`
- `signal output outL`, `signal output outR`

When `sel = 0`, outputs pass through; when `sel = 1`, outputs are swapped.
",
    },
    CircomlibEntry {
        name: "Mux1",
        markdown: "**Mux1()** — 1-bit multiplexer.

**Signals**
- `signal input c[2]`
- `signal input s` (selector, must be 0/1)
- `signal output out`
",
    },
    CircomlibEntry {
        name: "Mux2",
        markdown: "**Mux2()** — 2-bit multiplexer over 4 inputs. Selector must be range-checked to `[0,4)` by the caller.
",
    },
    CircomlibEntry {
        name: "Mux3",
        markdown: "**Mux3()** — 3-bit multiplexer over 8 inputs. Selector bits must be in `{0,1}`.
",
    },
    CircomlibEntry {
        name: "Mux4",
        markdown: "**Mux4()** — 4-bit multiplexer over 16 inputs. Selector bits must be in `{0,1}`.
",
    },
    CircomlibEntry {
        name: "MultiMux1",
        markdown: "**MultiMux1(n)** — `n`-wide 1-bit multiplexer.

**Signals**
- `signal input c[n][2]`
- `signal input s`
- `signal output out[n]`
",
    },
    CircomlibEntry {
        name: "XOR",
        markdown: "**XOR()** — boolean XOR of two {0,1} inputs.

**Signals**
- `signal input a`, `signal input b` (caller ensures binary)
- `signal output out` (`= a + b - 2ab`)
",
    },
    CircomlibEntry {
        name: "AND",
        markdown: "**AND()** — boolean AND of two {0,1} inputs (caller ensures binary).
",
    },
    CircomlibEntry {
        name: "OR",
        markdown: "**OR()** — boolean OR of two {0,1} inputs (caller ensures binary).
",
    },
    CircomlibEntry {
        name: "NOT",
        markdown: "**NOT()** — boolean NOT of a {0,1} input (`= 1 - in`; caller ensures binary).
",
    },
    CircomlibEntry {
        name: "NAND",
        markdown: "**NAND()** — boolean NAND of two {0,1} inputs.
",
    },
    CircomlibEntry {
        name: "NOR",
        markdown: "**NOR()** — boolean NOR of two {0,1} inputs.
",
    },
    CircomlibEntry {
        name: "MultiAND",
        markdown: "**MultiAND(n)** — boolean AND across `n` {0,1} inputs.
",
    },
    CircomlibEntry {
        name: "BinSum",
        markdown: "**BinSum(n, ops)** — bit-level adder tree that sums `ops` operands each `n` bits wide.
",
    },
    CircomlibEntry {
        name: "SMTVerifier",
        markdown: "**SMTVerifier(nLevels)** — sparse-Merkle-tree inclusion/exclusion proof verifier.

**Params**
- `nLevels`: tree depth.

Verifies that `key → value` is (or is not) present under a given root.
",
    },
    CircomlibEntry {
        name: "SMTProcessor",
        markdown: "**SMTProcessor(nLevels)** — insert, update, or delete a leaf in a sparse Merkle tree, producing the new root.
",
    },
    CircomlibEntry {
        name: "BabyAdd",
        markdown: "**BabyAdd()** — Baby Jubjub curve point addition.

**Signals**
- `signal input x1, y1, x2, y2`
- `signal output xout, yout`
",
    },
    CircomlibEntry {
        name: "BabyDbl",
        markdown: "**BabyDbl()** — Baby Jubjub curve point doubling.
",
    },
    CircomlibEntry {
        name: "BabyPbk",
        markdown: "**BabyPbk()** — Baby Jubjub public-key derivation from a scalar.

**Signals**
- `signal input in` (scalar)
- `signal output Ax, Ay` (point)
",
    },
    CircomlibEntry {
        name: "Sigma",
        markdown: "**Sigma()** — S-box used inside Poseidon (`x ↦ x^5`).
",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_templates_are_looked_up() {
        assert!(lookup("Num2Bits").is_some());
        assert!(lookup("IsZero").is_some());
        assert!(lookup("IsEqual").is_some());
        assert!(lookup("Poseidon").is_some());
        assert!(lookup("CompConstant").is_some());
    }

    #[test]
    fn unknown_template_returns_none() {
        assert!(lookup("MyCustomTemplate").is_none());
        assert!(lookup("").is_none());
        assert!(lookup("num2bits").is_none()); // case sensitive
    }

    #[test]
    fn all_entries_have_nonempty_markdown() {
        for entry in CIRCOMLIB_DOCS {
            assert!(!entry.name.is_empty());
            assert!(
                !entry.markdown.is_empty(),
                "empty markdown for {}",
                entry.name
            );
            assert!(
                entry.markdown.contains("**"),
                "no markdown emphasis in {}",
                entry.name
            );
        }
    }

    #[test]
    fn no_duplicate_names() {
        let mut names: Vec<&str> = CIRCOMLIB_DOCS.iter().map(|e| e.name).collect();
        names.sort();
        let original_len = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            original_len,
            "duplicate template names in CIRCOMLIB_DOCS"
        );
    }

    #[test]
    fn known_names_iterator_matches_table() {
        let count_iter = known_names().count();
        assert_eq!(count_iter, CIRCOMLIB_DOCS.len());
    }
}
