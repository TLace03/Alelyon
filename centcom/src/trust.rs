//! Trust: certified receipts verified on this PC by the project's own Rust verifier, and the words for what a
//! verdict says.
//!
//! The verifier is `alelyon_verify`, linked in (chosen 2026-10-03). It replays a receipt on the
//! substrate the build installed (`installed.rs`): the official builds install the private deterministic kernel; the
//! public build installs none, so a receipt is checked (signature, inputs, records) but not replayed, its number,
//! width, budget and tier checks are not performed, and it does not pass. The Python verifier remains the reference;
//! a W4 gate checks that the two agree.
//!
//! What a page may claim follows the CNE spec (section 9 of the CNE spec):
//! - a check is true, false or null, and null means it was NOT PERFORMED. It is never shown as passed;
//! - authenticity needs a public key obtained and pinned apart from the receipt, never the one the receipt carries;
//! - a valid refusal is evidence of refusal, not a certified number;
//! - the error terms (quantization, sampling, provider, model) are kept apart, and a missing term is not zero.

use std::path::Path;

use alelyon_verify::canonical::{JsonResourceLimits, parse_json_strict_bounded};
use alelyon_verify::{ReplayKernel, verifier};
use iced::widget::{Column, container, row};
use iced::{Alignment, Element};
use crate::theme;
use serde_json::{Map, Value};

use crate::ui::{chip, label, mono, strong, subheading};

/// The published CLI's bounds for one JSON file: 64 MiB, depth 64, 2 million nodes,
/// a million items in one container, 1 MiB in one string. Duplicate keys are refused by the parser.
const MAX_FILE_BYTES: u64 = 64 << 20;
const LIMITS: JsonResourceLimits =
    JsonResourceLimits { max_depth: 64, max_nodes: 2_000_000, max_container_items: 1_000_000, max_string_bytes: 1 << 20 };

/// The verdict's checks, in the spec's order.
pub const CHECKS: [&str; 11] =
    ["authenticity", "inputs", "scalar", "width", "budget", "program", "tier", "transparency", "witness", "key_status", "provider"];

/// A JSON file, read strictly within the bounds the `alelyon-verify` command applies to the same files.
pub fn read(path: &Path) -> Result<Value, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if size > MAX_FILE_BYTES {
        return Err(format!("{} is larger than 64 MiB", path.display()));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// JSON text, parsed strictly within the same bounds.
pub fn parse(text: &str) -> Result<Value, String> {
    parse_json_strict_bounded(text.trim_start_matches('\u{feff}'), LIMITS).map_err(|e| e.to_string())
}

/// What a file holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// A receipt (`alelyon.cne/v0`).
    Envelope(Value),
    /// A self-contained case, as the published test vectors are: a receipt, its data and the pinned keys.
    Case { envelope: Value, inputs: Option<Value>, public_key_hex: String, witness_key_hex: String },
    /// Data a receipt was computed from: a map of `kind|key` to series or tables.
    Data(Value),
    Other,
}

pub fn classify(v: &Value) -> Found {
    let is_envelope = |e: &Value| e.get("type").and_then(Value::as_str) == Some("alelyon.cne/v0");
    if is_envelope(v) {
        return Found::Envelope(v.clone());
    }
    if let Some(envelope) = v.get("envelope").filter(|e| is_envelope(e)) {
        let pin = |key: &str| v.get("pins").and_then(|p| p.get(key)).and_then(Value::as_str).unwrap_or("").to_string();
        return Found::Case {
            envelope: envelope.clone(),
            inputs: v.get("inputs").cloned().filter(|i| !i.is_null()),
            public_key_hex: pin("public_key_hex"),
            witness_key_hex: pin("witness_key_hex"),
        };
    }
    match v.as_object() {
        Some(map) if !map.is_empty() && map.keys().all(|k| k.contains('|')) => Found::Data(v.clone()),
        _ => Found::Other,
    }
}

/// A verdict, as the window shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub ok: bool,
    /// The receipt is a refusal: evidence the issuer refused, not a certified number.
    pub refused: bool,
    /// Each check: Some(true) passed, Some(false) failed, None NOT PERFORMED.
    pub checks: Vec<(&'static str, Option<bool>)>,
    /// The spec's reason classes, in the verifier's order.
    pub reasons: Vec<String>,
    pub width_trust: String,
    pub provider_trust: String,
    /// The build replayed it: false when no replay substrate is installed, and then the replay checks are not
    /// performed whatever else held.
    pub replayed: bool,
}

/// Verify `envelope` against its `data` with the keys a person pinned. An empty key is no pin: authenticity is then
/// not established, and the verdict says so.
pub fn verify(envelope: &Value, data: Option<&Value>, public_key_hex: &str, witness_key_hex: &str) -> Verdict {
    let mut pins = Map::new();
    for (name, key) in [("public_key_hex", public_key_hex), ("witness_key_hex", witness_key_hex)] {
        let key = key.trim().to_ascii_lowercase();
        if !key.is_empty() {
            pins.insert(name.to_string(), Value::String(key));
        }
    }
    let kernel = crate::installed::replay();
    verify_on(envelope, data, &Value::Object(pins), kernel.as_deref())
}

/// [`verify`] with the pins as the verifier takes them, replaying on `kernel` (None: not replayed).
pub fn verify_on(envelope: &Value, data: Option<&Value>, pins: &Value, kernel: Option<&dyn ReplayKernel>) -> Verdict {
    let raw = verifier::verify_envelope(envelope, data, Some(pins), kernel);
    let mut verdict = read_verdict(&raw, envelope);
    verdict.replayed = kernel.is_some();
    verdict
}

fn read_verdict(raw: &Value, envelope: &Value) -> Verdict {
    let text = |key: &str| raw.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let reasons = raw
        .get("reason_classes")
        .or_else(|| raw.get("reasons"))
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    Verdict {
        ok: raw.get("ok").and_then(Value::as_bool).unwrap_or(false),
        refused: envelope.get("refused").and_then(Value::as_bool).unwrap_or(false),
        checks: CHECKS.iter().map(|c| (*c, raw.get("checks").and_then(|k| k.get(*c)).and_then(Value::as_bool))).collect(),
        reasons,
        width_trust: text("width_trust"),
        provider_trust: text("provider_trust"),
        replayed: false,
    }
}

/// What a check covers, in a line.
pub fn check_text(check: &str) -> &'static str {
    match check {
        "authenticity" => "The signature matches the pinned key",
        "inputs" => "The data matches what the receipt committed to",
        "scalar" => "The number replays to the same value",
        "width" => "The stated width replays",
        "budget" => "The error budget replays",
        "program" => "The program matches its hash",
        "tier" => "The program's tier is the one claimed",
        "transparency" => "The data's capture log proves each row",
        "witness" => "An independent witness co-signed the log",
        "key_status" => "The key was valid when it signed",
        "provider" => "The data sources' corroboration record holds",
        _ => "",
    }
}

/// Reason classes that inform without changing the verdict (`ADVISORY_REASON_CLASSES`).
pub fn advisory(class: &str) -> bool {
    matches!(
        class,
        "unspecified-substrate" | "width-substrate-independent" | "scalar-tolerance-window" | "transparency-partial" | "witness-partial" | "witness-unpinned"
    )
}

/// A reason class in words. The classes are the spec's frozen vocabulary (`REASON_CLASSES`); the Rust verifier
/// reports the classes alone, so the sentences live here.
pub fn reason_text(class: &str) -> String {
    let words = match class {
        "not-a-cne-v0" => "This is not a certified number envelope (alelyon.cne/v0).",
        "no-pinned-key" => "No public key was pinned, so who signed it is not established.",
        "malformed-pinned-key" => "The pinned key is not 64 hexadecimal characters.",
        "unsigned" => "The receipt carries no signature.",
        "key-id-mismatch" => "The receipt names a different key than the one pinned.",
        "bad-signature" => "The signature does not match the pinned key: the receipt was altered or signed by someone else.",
        "program-hash-mismatch" => "The program does not match the hash the receipt committed to.",
        "malformed-envelope" => "The receipt is malformed.",
        "no-input-data" => "No data was given, so the number could not be replayed.",
        "input-missing" => "Some data the receipt used is missing.",
        "input-digest-mismatch" => "The data differs from what the receipt committed to.",
        "delta-count-mismatch" => "The receipt's quantization record does not match the data's rows.",
        "uncertified-count-mismatch" => "The count of uncertified rows does not match.",
        "replay-refusal-mismatch" => "Replaying gives a different refusal decision than the receipt states.",
        "scalar-mismatch" => "Replaying gives a different number.",
        "tier-mismatch" => "Replaying gives a different program tier.",
        "budget-mismatch" => "Replaying gives a different error budget.",
        "no-seed" => "The receipt carries no resampling seed, so its width cannot be replayed.",
        "width-mismatch" => "Replaying gives a different width.",
        "substrate-mismatch" => "The receipt was computed on a different numeric kernel; its width cannot be replayed here.",
        "unspecified-substrate" => "The receipt does not name its numeric kernel.",
        "width-substrate-independent" => "The width is exactly zero, which does not depend on the kernel.",
        "scalar-tolerance-window" => "The number matched within the allowed floating-point tolerance.",
        "transparency-no-pinned-key" => "The capture log's key is not pinned.",
        "transparency-partial" => "Only part of the data is proven by the capture log.",
        "anchor-sth-invalid" => "The capture log's signed head is invalid.",
        "anchor-malformed-scope" | "anchor-scope-mismatch" | "anchor-sth-scope-mismatch" => {
            "The capture log's proof is for a different scope than the data."
        }
        "anchor-malformed-leaf" | "anchor-leaf-hash-mismatch" => "A capture log entry does not hash to what was proven.",
        "anchor-proof-tree-size-mismatch" | "anchor-proof-index-out-of-range" | "anchor-inclusion-failed" => {
            "A capture log inclusion proof fails."
        }
        "anchor-delta-unusable" => "A capture log entry's quantization record is unusable.",
        "anchor-no-data" => "The capture log proof has no data to prove.",
        "anchor-length-mismatch" => "The capture log covers a different number of rows.",
        "anchor-row-uncovered" => "A row is not covered by the capture log.",
        "anchor-delta-mismatch" => "The capture log's quantization record differs from the receipt's.",
        "anchor-delta-zero-implausible" => "The capture log claims an implausible zero quantization.",
        "capture-uncertified-leaf" => "The issuer recorded that a capture produced no certificate.",
        "value-commitment-absent" => "A capture log entry makes no statement about the value.",
        "value-commitment-unopenable" => "A value commitment cannot be opened by this verifier.",
        "value-commitment-mismatch" => "A committed value differs from the data.",
        "witness-unpinned" => "No witness key was pinned, so the co-signature was not checked.",
        "witness-cosignature-invalid" => "The witness co-signature is invalid.",
        "witness-partial" => "Only part of the log is co-signed by the witness.",
        "witness-malformed" => "The witness co-signature is malformed.",
        "key-manifest-unrooted" | "key-manifest-invalid" => "The key manifest is not valid.",
        "key-not-in-manifest" => "The signing key is not in the key manifest.",
        "key-revoked" => "The signing key was revoked.",
        "key-outside-validity" => "The signing key was not valid when the receipt was made.",
        "key-manifest-checkpoint-required" | "key-manifest-checkpoint-invalid" | "key-manifest-checkpoint-not-monotonic" => {
            "The key manifest's checkpoint does not hold."
        }
        "provider-no-pinned-key" => "The data sources' corroboration log key is not pinned.",
        "provider-partial" => "Only part of the data is corroborated.",
        "provider-attempt-count-mismatch" | "provider-attempts-digest-mismatch" | "provider-outcome-unknown" => {
            "The corroboration attempts were altered or are incomplete."
        }
        "provider-summary-mismatch" => "The corroboration summary disagrees with its records.",
        other if other.starts_with("provider-") => "The data sources' corroboration proof fails.",
        _ => "",
    };
    if words.is_empty() { format!("{class} (no description: a class this page does not know)") } else { words.to_string() }
}

/// What a trust label means. The verifier gives the width and the data sources separate labels, and the
/// same word says a different thing of each: "transparency-anchored" is the capture log for the width, the
/// corroboration log for the data sources.
pub fn width_trust_text(label: &str) -> &'static str {
    match label {
        "authenticated" => "the width is replayed and signed by the pinned key",
        "transparency-anchored" => "the width is proven against the signed capture log",
        "unverified" => "the width is not established",
        "refusal" => "a refusal carries no width",
        _ => "",
    }
}

pub fn provider_trust_text(label: &str) -> &'static str {
    match label {
        "transparency-anchored" => "which sources were asked, and what each said, is proven against the signed corroboration log",
        "signer-attested" => "only the signer vouches for the data sources",
        _ => "",
    }
}

/// What the receipt states (the issuer's declaration, not this PC's check): its number and each error term.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stated {
    pub program: String,
    pub scalar: Option<String>,
    pub key_id: String,
    /// (term, what it says). A term stated as null is "not stated", never zero.
    pub terms: Vec<(String, String)>,
}

pub fn stated(envelope: &Value) -> Stated {
    let text = |key: &str| envelope.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut terms = Vec::new();
    if let Some(budget) = envelope.get("error_budget").and_then(Value::as_object) {
        for name in ["quantization", "sampling", "provider", "model"] {
            let says = match budget.get(name) {
                None | Some(Value::Null) => "not stated (a missing term is not zero)".to_string(),
                Some(term) => {
                    let mut parts = Vec::new();
                    if let Some(w) = term.get("width").filter(|w| !w.is_null()) {
                        parts.push(format!("width {w}"));
                    }
                    if let Some(tier) = term.get("tier").and_then(Value::as_str) {
                        parts.push(format!("tier {tier}"));
                    }
                    if term.get("exact").and_then(Value::as_bool) == Some(true) {
                        parts.push("exact".into());
                    }
                    if let Some(status) = term.get("status").and_then(Value::as_str) {
                        parts.push(status.to_uppercase());
                    }
                    if parts.is_empty() { "stated".into() } else { parts.join(", ") }
                }
            };
            terms.push((name.to_string(), says));
        }
    }
    Stated {
        program: text("program"),
        scalar: envelope.get("scalar").filter(|s| !s.is_null()).map(|s| s.to_string()),
        key_id: text("key_id"),
        terms,
    }
}

/// The checks, the reasons, what the width and the data sources rest on, and what the receipt states, below `card`:
/// the Verify page's verdict, and a plug-in's card about a receipt it made.
pub fn verdict_body<'a, M: 'a>(card: Column<'a, M>, v: &'a Verdict, s: &'a Stated) -> Column<'a, M> {
    // said first, above the checks it explains, so it is read before the column of "Not performed"
    let mut card = if v.replayed { card } else { card.push(label(crate::installed::REPLAY_NOT_IN_BUILD, 13.0, theme::CAUTION)) };
    card = card.push(subheading("What this PC checked"));
    for (check, state) in &v.checks {
        let status: Element<'a, M> = match state {
            Some(true) => chip("Passed", theme::POSITIVE),
            Some(false) => chip("Failed", theme::DANGER),
            None => chip("Not performed", theme::TEXT_FAINT),
        };
        card = card.push(
            row![container(status).width(118), strong(*check, 13.0, theme::TEXT).width(110), label(check_text(check), 13.0, theme::TEXT_DIM)]
                .spacing(10)
                .align_y(Alignment::Center),
        );
    }
    card = card.push(label("Not performed is never a pass: that check said nothing about this receipt.", 12.0, theme::TEXT_FAINT));
    if !v.reasons.is_empty() {
        card = card.push(subheading("Why"));
        for class in &v.reasons {
            let (lead, color) = if advisory(class) { ("Note", theme::TEXT_DIM) } else { ("Reason", theme::CAUTION) };
            card = card.push(
                row![container(label(lead, 12.0, color)).width(60), label(reason_text(class), 13.0, theme::TEXT), mono(class.as_str(), 11.0, theme::TEXT_FAINT)]
                    .spacing(10)
                    .align_y(Alignment::Center),
            );
        }
    }
    for (name, value, meaning) in [
        ("The width", &v.width_trust, width_trust_text(&v.width_trust)),
        ("The data sources", &v.provider_trust, provider_trust_text(&v.provider_trust)),
    ] {
        if !value.is_empty() {
            let said = if meaning.is_empty() { format!("{name}: {value}.") } else { format!("{name}: {value} ({meaning}).") };
            card = card.push(label(said, 13.0, theme::TEXT_DIM));
        }
    }
    card = card.push(subheading("What the receipt states"));
    card = card.push(label("The issuer's own declaration, shown as written; what holds of it is the checks above.", 12.0, theme::TEXT_FAINT));
    if !s.program.is_empty() {
        card = card.push(row![container(label("Program", 12.5, theme::TEXT_DIM)).width(110), mono(s.program.as_str(), 12.5, theme::TEXT)].spacing(10));
    }
    let number = s.scalar.clone().unwrap_or_else(|| "none (a refusal)".into());
    card = card.push(row![container(label("Number", 12.5, theme::TEXT_DIM)).width(110), mono(number, 12.5, theme::TEXT)].spacing(10));
    if !s.key_id.is_empty() {
        card = card.push(row![container(label("Signed by", 12.5, theme::TEXT_DIM)).width(110), mono(s.key_id.as_str(), 12.5, theme::TEXT)].spacing(10));
    }
    for (term, says) in &s.terms {
        card = card.push(row![container(label(format!("{term} term"), 12.5, theme::TEXT_DIM)).width(110), label(says.as_str(), 12.5, theme::TEXT)].spacing(10));
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vectors() -> Vec<(String, Value)> {
        // the verifier crate's copy of the published conformance vectors
        let dir = PathBuf::from(alelyon_verify::TEST_VECTORS);
        let mut out: Vec<(String, Value)> = std::fs::read_dir(&dir)
            .expect("the published test vectors")
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
            .map(|e| (e.file_name().to_string_lossy().to_string(), read(&e.path()).expect("a vector reads within the bounds")))
            // manifest.json indexes the suite; a vector is a case with an expected verdict
            .filter(|(_, v)| v.get("expect").is_some_and(Value::is_object))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The public build: the vector's own pins, the linked verifier and no replay substrate, exactly as the app
    /// reaches it. No vector passes, the four replay checks are not performed, and every other check comes out as
    /// the vector expects. (The official build's tests run the same vectors on the kernel, where each gets its whole
    /// expected verdict.)
    #[test]
    fn every_published_vector_is_checked_but_not_replayed_and_none_passes_in_the_public_build() {
        let all = vectors();
        assert!(all.len() >= 50, "{} vectors", all.len());
        assert!(crate::installed::replay().is_none(), "no centcom test installs a substrate");
        for (name, case) in &all {
            let pins = case.get("pins").cloned().unwrap_or(Value::Null);
            let got = verify_on(&case["envelope"], case.get("inputs").filter(|i| !i.is_null()), &pins, None);
            let expect = &case["expect"];
            assert!(!got.ok && !got.replayed, "{name}: not replayed, not passed");
            for (check, state) in &got.checks {
                if ["scalar", "width", "budget", "tier"].contains(check) {
                    assert_eq!(*state, None, "{name}: {check} is not performed without the replay engine");
                } else {
                    assert_eq!(*state, expect["checks"][*check].as_bool(), "{name}: {check} (null = not performed)");
                }
            }
        }
    }

    #[test]
    fn a_receipt_is_authentic_only_against_a_key_pinned_apart_from_it() {
        let case = vectors().into_iter().find(|(n, _)| n == "golden-authenticated.json").unwrap().1;
        let Found::Case { envelope, inputs, public_key_hex, .. } = classify(&case) else { panic!("a case") };
        let pinned = verify(&envelope, inputs.as_ref(), &public_key_hex, "");
        // authentic against the pin; not passed, as the public build does not replay it
        assert_eq!(pinned.checks[0], ("authenticity", Some(true)), "{pinned:?}");
        assert!(!pinned.ok && !pinned.replayed);
        // the receipt carries its own public key; without a pin it is still not authentic (the spec: a key obtained
        // apart from the receipt), and the verifier says why
        let unpinned = verify(&envelope, inputs.as_ref(), "", "");
        assert!(!unpinned.ok);
        assert_eq!(unpinned.checks[0], ("authenticity", Some(false)), "no pin: authenticity is not established");
        assert!(unpinned.reasons.iter().any(|r| r == "no-pinned-key"), "{:?}", unpinned.reasons);
        let wrong = verify(&envelope, inputs.as_ref(), &"0".repeat(64), "");
        assert!(!wrong.ok);
        assert_eq!(wrong.checks[0].1, Some(false));
    }

    #[test]
    fn a_dropped_file_is_recognised_for_what_it_holds() {
        let case = vectors().into_iter().find(|(n, _)| n == "golden-anchored.json").unwrap().1;
        assert!(matches!(classify(&case), Found::Case { .. }));
        assert!(matches!(classify(&case["envelope"]), Found::Envelope(_)));
        assert!(matches!(classify(&case["inputs"]), Found::Data(_)));
        assert_eq!(classify(&serde_json::json!({"hello": 1})), Found::Other);
    }

    #[test]
    fn what_the_receipt_states_keeps_each_term_apart_and_a_missing_term_is_not_zero() {
        let case = vectors().into_iter().find(|(n, _)| n == "golden-authenticated.json").unwrap().1;
        let s = stated(&case["envelope"]);
        assert_eq!(s.program, "show mean(price(\"SYN\"))");
        assert!(s.scalar.as_deref().unwrap_or("").starts_with("99.6"), "{:?}", s.scalar);
        let term = |name: &str| s.terms.iter().find(|(t, _)| t == name).map(|(_, w)| w.clone()).unwrap();
        assert!(term("quantization").contains("width") && term("quantization").contains("exact"));
        assert_eq!(term("sampling"), "not stated (a missing term is not zero)");
        assert!(term("provider").contains("UNMEASURED"), "{}", term("provider"));
    }

    #[test]
    fn every_reason_class_the_vectors_use_has_words() {
        for (name, case) in vectors() {
            for class in case["expect_classes"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                assert!(!reason_text(class).contains("no description"), "{name}: {class} has no words");
            }
        }
    }

    #[test]
    fn every_trust_label_the_verifier_gives_has_words_for_what_it_labels() {
        for (name, case) in vectors() {
            let pins = case.get("pins").cloned().unwrap_or(Value::Null);
            let got = verify_on(&case["envelope"], case.get("inputs").filter(|i| !i.is_null()), &pins, None);
            assert!(!width_trust_text(&got.width_trust).is_empty(), "{name}: width {}", got.width_trust);
            let sources = provider_trust_text(&got.provider_trust);
            assert!(!sources.is_empty(), "{name}: data sources {}", got.provider_trust);
            assert!(!sources.contains("width"), "{name}: the data sources' label is not about the width");
        }
    }

    #[test]
    fn a_file_over_the_bounds_is_refused_before_it_is_parsed() {
        let dir = std::env::temp_dir().join(format!("centcom-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let deep = dir.join("deep.json");
        std::fs::write(&deep, "[".repeat(100) + &"]".repeat(100)).unwrap();
        assert!(read(&deep).is_err(), "depth 100 is over the bound of 64");
        let dup = dir.join("dup.json");
        std::fs::write(&dup, r#"{"a": 1, "a": 2}"#).unwrap();
        assert!(read(&dup).is_err(), "duplicate keys are refused");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
