//! The C++ core against Python's own answers.
//!
//! Every file under `tests/goldens/` was written by `tools/model_anatomy_goldens.py`
//! running the real Python (`morphometry.analyze`, `voxel_field`,
//! `canonical_space`, `space_commitment`, `GGUFHeader.show_payload`). Each is
//! compared here structurally: same keys, same array lengths, strings byte for
//! byte, integers exactly, floats by their bits, an integer never standing in
//! for a float or the reverse.

use std::fs;
use std::path::{Path, PathBuf};

use model_anatomy::gguf::{GgufTensor, GgufValue, show_payload};
use model_anatomy::{MetaValue, Payload, PayloadTensor};
use serde_json::Value;

fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

fn read(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    expand(value)
}

/// Undoes the golden writer's `{"$repeat": item, "$count": n}` compaction.
fn expand(value: Value) -> Value {
    match value {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Object(ref map)
                        if map.contains_key("$repeat") && map.contains_key("$count") =>
                    {
                        let count = map["$count"].as_u64().expect("$count") as usize;
                        let one = expand(map["$repeat"].clone());
                        out.extend(std::iter::repeat_n(one, count));
                    }
                    other => out.push(expand(other)),
                }
            }
            Value::Array(out)
        }
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, expand(v))).collect()),
        other => other,
    }
}

fn files(folder: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(goldens().join(folder))
        .expect("golden folder")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    out.sort();
    out
}

/// Every difference between `actual` and `expected`, by JSON path.
fn compare(actual: &Value, expected: &Value, path: &str, out: &mut Vec<String>) {
    match (actual, expected) {
        (Value::Null, Value::Null) => {}
        (Value::Bool(a), Value::Bool(e)) if a == e => {}
        (Value::String(a), Value::String(e)) if a.as_bytes() == e.as_bytes() => {}
        (Value::Number(a), Value::Number(e)) => {
            let same = if e.is_f64() {
                a.is_f64() && a.as_f64().map(f64::to_bits) == e.as_f64().map(f64::to_bits)
            } else if e.is_i64() {
                !a.is_f64() && a.as_i64() == e.as_i64()
            } else {
                !a.is_f64() && a.as_u64() == e.as_u64()
            };
            if !same {
                out.push(format!("{path}: {a} != expected {e}"));
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            if a.len() != e.len() {
                out.push(format!("{path}: {} items != expected {}", a.len(), e.len()));
                return;
            }
            for (index, (x, y)) in a.iter().zip(e).enumerate() {
                compare(x, y, &format!("{path}[{index}]"), out);
            }
        }
        (Value::Object(a), Value::Object(e)) => {
            for key in e.keys() {
                match a.get(key) {
                    Some(x) => compare(x, &e[key], &format!("{path}.{key}"), out),
                    None => out.push(format!("{path}.{key}: missing")),
                }
            }
            for key in a.keys().filter(|k| !e.contains_key(*k)) {
                out.push(format!("{path}.{key}: not expected"));
            }
        }
        _ => out.push(format!("{path}: {actual} != expected {expected}")),
    }
}

fn assert_same(actual: &Value, expected: &Value, what: &str) {
    let mut differences = Vec::new();
    compare(actual, expected, "$", &mut differences);
    assert!(
        differences.is_empty(),
        "{what}: {} difference(s):\n  {}",
        differences.len(),
        differences
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

fn text(value: &Value) -> String {
    value.as_str().expect("string").to_owned()
}

fn meta_value(tagged: &Value) -> MetaValue {
    let (kind, value) = tagged
        .as_object()
        .expect("tagged")
        .iter()
        .next()
        .expect("one tag");
    match kind.as_str() {
        "none" => MetaValue::None,
        "bool" => MetaValue::Bool(value.as_bool().expect("bool")),
        "int" => MetaValue::Int(text(value)),
        "f64" => MetaValue::F64(text(value).parse().expect("float repr")),
        "str" => MetaValue::Str(text(value)),
        other => panic!("unknown tag {other}"),
    }
}

fn tensor_of(item: &Value) -> PayloadTensor {
    PayloadTensor {
        name: text(&item["name"]),
        shape: item["shape"]
            .as_array()
            .expect("shape")
            .iter()
            .map(|d| d.as_u64().expect("dim"))
            .collect(),
        type_name: text(&item["type"]),
    }
}

fn payload_of(encoded: &Value) -> Option<Payload> {
    if encoded.is_null() {
        return None;
    }
    let tensors = encoded["tensors"].as_array().map(|items| {
        let mut out = Vec::new();
        for item in items {
            match item.get("$each") {
                Some(count) => {
                    for index in 0..count.as_u64().expect("$each") {
                        let mut one = tensor_of(item);
                        one.name = one.name.replace("{}", &index.to_string());
                        out.push(one);
                    }
                }
                None => out.push(tensor_of(item)),
            }
        }
        out
    });
    Some(Payload {
        model: text(&encoded["model"]),
        family: text(&encoded["family"]),
        quantization_level: text(&encoded["quantization_level"]),
        parent_model: text(&encoded["parent_model"]),
        model_info: encoded["model_info"]
            .as_array()
            .expect("model_info")
            .iter()
            .map(|pair| (text(&pair[0]), meta_value(&pair[1])))
            .collect(),
        tensors,
    })
}

#[test]
fn every_analyze_golden_matches_python() {
    let paths = files("analyze");
    // A missing folder or an emptied one must not pass as agreement.
    assert!(paths.len() >= 70, "only {} analyze goldens", paths.len());
    let mut failures = Vec::new();
    for path in &paths {
        let golden = read(path);
        let input = &golden["input"];
        let payload = payload_of(&input["payload"]);
        let model = text(&input["model"]);
        let actual = model_anatomy::analyze_json(payload.as_ref(), &model).expect("analysis");
        let actual: Value = serde_json::from_str(&actual).expect("core JSON");
        let mut differences = Vec::new();
        compare(&actual, &golden["expected"], "$", &mut differences);
        if !differences.is_empty() {
            failures.push(format!(
                "{}:\n    {}",
                path.file_name().unwrap().to_string_lossy(),
                differences
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
        // The typed layer accepts every shape the goldens hold.
        model_anatomy::analyze(payload.as_ref(), &model).expect("typed analysis");
    }
    assert!(
        failures.is_empty(),
        "{} of {} goldens differ:\n  {}",
        failures.len(),
        paths.len(),
        failures.join("\n  ")
    );
}

fn header_value(tagged: &Value) -> GgufValue {
    let map = tagged.as_object().expect("tagged");
    if map.contains_key("list") {
        return GgufValue::Other(text(&map["text"]));
    }
    match meta_value(tagged) {
        MetaValue::Bool(flag) => GgufValue::Bool(flag),
        MetaValue::Int(digits) => GgufValue::Int(digits),
        MetaValue::F64(number) => GgufValue::Float(number),
        MetaValue::Str(text) => GgufValue::Str(text),
        MetaValue::None => panic!("a header holds no None"),
    }
}

fn tag_of(value: &MetaValue) -> Value {
    match value {
        MetaValue::None => serde_json::json!({"none": null}),
        MetaValue::Bool(flag) => serde_json::json!({"bool": flag}),
        MetaValue::Int(digits) => serde_json::json!({"int": digits}),
        MetaValue::F64(number) => {
            serde_json::json!({"f64": model_anatomy::format_repr(*number).unwrap()})
        }
        MetaValue::Str(text) => serde_json::json!({"str": text}),
    }
}

#[test]
fn every_show_payload_golden_matches_python() {
    let paths = files("show_payload");
    assert!(
        paths.len() >= 10,
        "only {} show_payload goldens",
        paths.len()
    );
    for path in &paths {
        let golden = read(path);
        let input = &golden["input"];
        let metadata: Vec<(String, GgufValue)> = input["metadata"]
            .as_array()
            .expect("metadata")
            .iter()
            .map(|pair| (text(&pair[0]), header_value(&pair[1])))
            .collect();
        let tensors: Vec<GgufTensor> = input["tensors"]
            .as_array()
            .expect("tensors")
            .iter()
            .map(|t| GgufTensor {
                name: text(&t["name"]),
                shape: t["shape"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| d.as_u64().unwrap())
                    .collect(),
                ggml_type: t["ggml_type"].as_u64().unwrap() as u32,
            })
            .collect();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let shown = show_payload(&text(&input["model"]), &metadata, &tensors);
        let expected = &golden["expected"];
        if let Some(error) = expected.get("error") {
            assert_eq!(
                shown.map(|_| ()).map_err(|e| e.kind()),
                Err(error.as_str().unwrap()),
                "{name}"
            );
            continue;
        }
        let shown = shown.unwrap_or_else(|e| panic!("{name}: {}", e.kind()));
        let as_json = serde_json::json!({
            "model": shown.model,
            "family": shown.family,
            "quantization_level": shown.quantization_level,
            "model_info": shown.model_info.iter().map(|(k, v)| serde_json::json!([k, tag_of(v)])).collect::<Vec<_>>(),
            "tensors": shown.tensors.as_ref().unwrap().iter().map(|t| serde_json::json!({
                "name": t.name, "shape": t.shape, "type": t.type_name})).collect::<Vec<_>>(),
        });
        assert_same(&as_json, &expected["payload"], &format!("{name} payload"));
        assert!(shown.parent_model.is_empty());
        let actual: Value = serde_json::from_str(
            &model_anatomy::analyze_json(Some(&shown), &text(&input["model"])).unwrap(),
        )
        .unwrap();
        assert_same(&actual, &expected["analysis"], &format!("{name} analysis"));
    }
}

#[test]
fn the_constants_match_python() {
    let actual: Value = serde_json::from_str(&model_anatomy::constants_json().unwrap()).unwrap();
    assert_same(
        &actual,
        &read(&goldens().join("constants.json")),
        "constants",
    );
}

#[test]
fn the_canonical_space_and_its_commitment_match_python() {
    let actual: Value =
        serde_json::from_str(&model_anatomy::canonical_space_json().unwrap()).unwrap();
    assert_same(
        &actual,
        &read(&goldens().join("canonical_space.json")),
        "canonical space",
    );
    let typed = model_anatomy::canonical_space().unwrap();
    assert!(typed.commitment.starts_with("sha256:") && typed.commitment.len() == 71);
}

#[test]
fn the_formatters_match_python() {
    let golden = read(&goldens().join("format.json"));
    let mut checked = 0;
    for row in golden["repr"].as_array().unwrap() {
        let x: f64 = text(&row[0]).parse().unwrap();
        assert_eq!(
            model_anatomy::format_repr(x).unwrap(),
            text(&row[1]),
            "repr {}",
            row[0]
        );
        checked += 1;
    }
    for row in golden["fixed"].as_array().unwrap() {
        let x: f64 = text(&row[0]).parse().unwrap();
        let n = row[1].as_i64().unwrap() as i32;
        assert_eq!(
            model_anatomy::format_fixed(x, n).unwrap(),
            text(&row[2]),
            "fixed {} {n}",
            row[0]
        );
        checked += 1;
    }
    for row in golden["grouped"].as_array().unwrap() {
        let n = row[0].as_i64().unwrap();
        assert_eq!(
            model_anatomy::format_grouped(n).unwrap(),
            text(&row[1]),
            "grouped {n}"
        );
        checked += 1;
    }
    assert!(checked >= 60, "only {checked} format rows");
}

// ── PR 2: registration, the template hierarchy, the lineage layers ──────────

use model_anatomy::{CoordinateAxis, CoordinateSpace, TemplateNode, TemplateRef};

fn opt_text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn texts(value: &Value) -> Vec<String> {
    value.as_array().expect("list").iter().map(text).collect()
}

fn text_pairs(value: &Value) -> Vec<(String, String)> {
    value
        .as_array()
        .expect("pairs")
        .iter()
        .map(|pair| (text(&pair[0]), text(&pair[1])))
        .collect()
}

fn opt_pair(value: &Value) -> Option<(String, String)> {
    value
        .as_array()
        .map(|pair| (text(&pair[0]), text(&pair[1])))
}

fn space_of(encoded: &Value) -> CoordinateSpace {
    CoordinateSpace {
        space_id: text(&encoded["space_id"]),
        version: text(&encoded["version"]),
        topology: text(&encoded["topology"]),
        axes: encoded["axes"]
            .as_array()
            .expect("axes")
            .iter()
            .map(|a| CoordinateAxis {
                axis_id: text(&a["axis_id"]),
                semantic_id: text(&a["semantic_id"]),
                kind: text(&a["kind"]),
                scalar_type: text(&a["scalar_type"]),
                ordering: text(&a["ordering"]),
                unit: opt_text(&a["unit"]),
                reference_frame: opt_text(&a["reference_frame"]),
                calendar: opt_text(&a["calendar"]),
                timezone: opt_text(&a["timezone"]),
                orientation: opt_text(&a["orientation"]),
                origin: opt_text(&a["origin"]),
                resolution: opt_text(&a["resolution"]),
                bounds: opt_pair(&a["bounds"]),
                periodicity: opt_pair(&a["periodicity"]),
                labels_ref: opt_text(&a["labels_ref"]),
                labels: a["labels"].as_array().map(|_| texts(&a["labels"])),
                missingness_policy: text(&a["missingness_policy"]),
                interpolation_policy: texts(&a["interpolation_policy"]),
                transform_policy: texts(&a["transform_policy"]),
                metadata: text_pairs(&a["metadata"]),
            })
            .collect(),
        index_convention: text(&encoded["index_convention"]),
        unit_system: opt_text(&encoded["unit_system"]),
        reference_frame: opt_text(&encoded["reference_frame"]),
        valid_domain_rule: text(&encoded["valid_domain_rule"]),
        region_atlas_refs: texts(&encoded["region_atlas_refs"]),
        metadata: text_pairs(&encoded["metadata"]),
    }
}

fn ref_pair(value: &Value) -> TemplateRef {
    TemplateRef {
        template_id: text(&value[0]),
        version: text(&value[1]),
    }
}

fn nodes_of(encoded: &Value) -> Option<Vec<TemplateNode>> {
    encoded.as_array().map(|nodes| {
        nodes
            .iter()
            .map(|n| TemplateNode {
                template_ref: ref_pair(&n["ref"]),
                tier: text(&n["tier"]),
                label: text(&n["label"]),
                description: text(&n["description"]),
                parent_refs: n["parent_refs"]
                    .as_array()
                    .expect("parents")
                    .iter()
                    .map(ref_pair)
                    .collect(),
                coordinate_space: (!n["coordinate_space"].is_null())
                    .then(|| space_of(&n["coordinate_space"])),
                transform_policy: texts(&n["transform_policy"]),
                gaps: texts(&n["gaps"]),
            })
            .collect()
    })
}

fn check_all(folder: &str, at_least: usize, run: impl Fn(&Value) -> String) {
    let paths = files(folder);
    assert!(
        paths.len() >= at_least,
        "only {} {folder} goldens",
        paths.len()
    );
    let mut failures = Vec::new();
    for path in &paths {
        let golden = read(path);
        let actual: Value = serde_json::from_str(&run(&golden["input"])).expect("core JSON");
        let mut differences = Vec::new();
        compare(&actual, &golden["expected"], "$", &mut differences);
        if !differences.is_empty() {
            failures.push(format!(
                "{}:\n    {}",
                path.file_name().unwrap().to_string_lossy(),
                differences
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} {folder} goldens differ:\n  {}",
        failures.len(),
        paths.len(),
        failures.join("\n  ")
    );
}

#[test]
fn every_register_golden_matches_python() {
    check_all("register", 15, |input| {
        let budget = input["edge_visit_budget"].as_i64().expect("budget");
        let source = space_of(&input["source"]);
        let target = space_of(&input["target"]);
        if budget == model_anatomy::MAX_MATCHING_EDGE_VISITS {
            model_anatomy::register(&source, &target).expect("typed report");
        }
        model_anatomy::register_json(&source, &target, budget).expect("report")
    });
}

#[test]
fn every_register_model_golden_matches_python() {
    check_all("register_model", 5, |input| {
        let payload = payload_of(&input["payload"]);
        let model = text(&input["model"]);
        model_anatomy::register_model(payload.as_ref(), &model).expect("typed report");
        model_anatomy::register_model_json(payload.as_ref(), &model).expect("report")
    });
}

#[test]
fn every_hierarchy_golden_matches_python() {
    check_all("hierarchy", 9, |input| {
        let nodes = nodes_of(&input["nodes"]);
        model_anatomy::template_hierarchy(nodes.as_deref()).expect("typed snapshot");
        model_anatomy::template_hierarchy_json(nodes.as_deref()).expect("snapshot")
    });
}

#[test]
fn the_node_budget_matches_python() {
    let golden = read(&goldens().join("hierarchy_budget.json"));
    let leaves = golden["input"]["root_then_leaves"].as_u64().unwrap();
    let node = |id: String, tier: &str, parents: Vec<TemplateRef>| TemplateNode {
        template_ref: TemplateRef {
            template_id: id,
            version: "1".into(),
        },
        tier: tier.into(),
        label: "L".into(),
        description: "D".into(),
        parent_refs: parents,
        coordinate_space: None,
        transform_policy: vec![],
        gaps: vec![],
    };
    let root = TemplateRef {
        template_id: "root".into(),
        version: "1".into(),
    };
    let mut nodes = vec![node("root".into(), "BASE_CONTRACT", vec![])];
    for i in 0..leaves {
        nodes.push(node(
            format!("leaf{i:04}"),
            "RESOLUTION_VARIANT",
            vec![root.clone()],
        ));
    }
    let snapshot: Value =
        serde_json::from_str(&model_anatomy::template_hierarchy_json(Some(&nodes)).unwrap())
            .unwrap();
    let nodes_out = snapshot["nodes"].as_array().unwrap();
    let actual = serde_json::json!({
        "state": snapshot["state"],
        "node_count": nodes_out.len(),
        "findings": snapshot["findings"],
        "last_node": nodes_out.last().unwrap()["ref"],
    });
    assert_same(&actual, &golden["expected"], "node budget");
}

#[test]
fn every_layers_golden_matches_python() {
    check_all("layers", 9, |input| {
        let nodes = nodes_of(&input["nodes"]);
        let payload = payload_of(&input["payload"]);
        let model = text(&input["model"]);
        let morph = input["measure"]
            .as_bool()
            .expect("measure")
            .then_some((payload.as_ref(), model.as_str()));
        let typed = model_anatomy::measure_layers(morph, nodes.as_deref()).expect("typed layers");
        // One volume, however many frames draw it.
        assert!(typed.volumes_built <= 1);
        model_anatomy::measure_layers_json(morph, nodes.as_deref()).expect("layers")
    });
}

// ── PR 3: the Foundry ───────────────────────────────────────────────────────

use model_anatomy::{FoundryModel, Gpu, Reading};

/// The core's document with every `native` object (what only the port says:
/// the D9 refusal, the reserve's basis in words) taken out, so the rest is
/// compared with Python's whole.
fn without_native(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| key != "native")
                .map(|(key, item)| (key, without_native(item)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(without_native).collect()),
        other => other,
    }
}

fn opt_i64(value: &Value) -> Option<i64> {
    value.as_i64()
}

fn reading_of(encoded: &Value) -> Reading {
    Reading {
        backend: text(&encoded["backend"]),
        torch_version: text(&encoded["torch_version"]),
        gpu: (!encoded["gpu"].is_null()).then(|| Gpu {
            name: text(&encoded["gpu"]["name"]),
            total_bytes: opt_i64(&encoded["gpu"]["total_bytes"]),
            free_bytes: opt_i64(&encoded["gpu"]["free_bytes"]),
            ..Gpu::default()
        }),
        ram_total_bytes: opt_i64(&encoded["ram_total_bytes"]),
        disk_free_bytes: opt_i64(&encoded["disk_free_bytes"]),
        bf16: encoded["bf16"].as_bool(),
        gaps: texts(&encoded["gaps"]),
        ..Reading::default()
    }
}

fn analyze_input(name: &str) -> Value {
    read(&goldens().join("analyze").join(format!("{name}.json")))["input"].clone()
}

#[test]
fn every_shape_golden_matches_python() {
    check_all("shape", 90, |input| {
        let input = match input.get("analyze_case") {
            Some(name) => analyze_input(name.as_str().expect("case name")),
            None => input.clone(),
        };
        let payload = payload_of(&input["payload"]);
        let model = text(&input["model"]);
        let typed = model_anatomy::shape(payload.as_ref(), &model).expect("typed shape");
        assert!(
            typed.native.refusal.is_empty(),
            "no golden holds a D9 refusal"
        );
        let json = model_anatomy::shape_json(payload.as_ref(), &model).expect("shape");
        without_native(serde_json::from_str(&json).expect("core JSON")).to_string()
    });
}

#[test]
fn every_foundry_golden_matches_python() {
    check_all("foundry", 40, |input| {
        let reading = reading_of(&input["reading"]);
        let models: Vec<FoundryModel> = input["models"]
            .as_array()
            .expect("models")
            .iter()
            .map(|m| FoundryModel {
                name: text(&m["name"]),
                payload: payload_of(&m["payload"]),
                raised: opt_text(&m["raised"]),
                requires_backends: texts(&m["requires_backends"]),
            })
            .collect();
        let context = input["context"].as_i64().expect("context");
        let fraction = opt_text(&input["mem_fraction"]);
        let (source, as_of) = (text(&input["source"]), text(&input["as_of"]));
        let typed = model_anatomy::foundry(
            &reading,
            &models,
            context,
            fraction.as_deref(),
            &source,
            &as_of,
        )
        .expect("typed foundry");
        assert!(
            typed.native.refusal.is_empty(),
            "no golden holds a D9 refusal"
        );
        assert!(
            !typed.native.reserve_basis.is_empty(),
            "the reserve says where it came from"
        );
        let json = model_anatomy::foundry_json(
            &reading,
            &models,
            context,
            fraction.as_deref(),
            &source,
            &as_of,
        )
        .expect("foundry");
        without_native(serde_json::from_str(&json).expect("core JSON")).to_string()
    });
}

#[test]
fn every_describe_golden_matches_python() {
    check_all("describe", 7, |input| {
        let reading = reading_of(input);
        model_anatomy::describe(&reading).expect("typed description");
        model_anatomy::describe_json(&reading).expect("description")
    });
}

#[test]
fn the_transform_facts_cover_nine_families_and_agree_with_the_ported_chains() {
    let facts = model_anatomy::transform_facts().expect("facts");
    assert_eq!(
        facts.families.len(),
        9,
        "transforms.py declares nine families"
    );
    let rank: std::collections::HashMap<&str, i64> = facts
        .loss_class_rank
        .iter()
        .map(|(name, rank)| (name.as_str(), *rank))
        .collect();
    for family in &facts.families {
        assert_eq!(
            rank[family.loss_class.as_str()],
            family.loss_rank,
            "{}",
            family.class_name
        );
        assert!(
            !family.doc.is_empty(),
            "{} has no docstring",
            family.class_name
        );
    }
    // The two families the core builds report what the table says they declare.
    for (golden, name) in [
        ("register/model_block_major.json", "IDENTITY"),
        ("register/model_module_major.json", "AXIS_PERMUTATION"),
    ] {
        let golden = read(&goldens().join(golden));
        let step = &golden["expected"]["transform"]["transforms"][0];
        let family = facts
            .families
            .iter()
            .find(|f| f.transform_type == name)
            .expect("family in table");
        assert_eq!(step["transform_type"], name);
        assert_eq!(
            step["loss_class"].as_str(),
            Some(family.loss_class.as_str())
        );
        assert_eq!(
            step["invertibility"].as_str(),
            Some(family.invertibility.as_str())
        );
    }
}

// ── PR 5: the economics ─────────────────────────────────────────────────────

use model_anatomy::{
    EnergyDeclaration, LlamacppResponse, MeterOutcome, MonthlyVolume, ProviderPrice,
    ThroughputReading,
};

/// An input float: its Python repr, or null for None.
fn in_float(value: &Value) -> Option<f64> {
    value
        .as_str()
        .map(|text| text.parse().unwrap_or_else(|_| panic!("float repr {text}")))
}

fn throughput_in(rec: &Value) -> ThroughputReading {
    let mut reading = ThroughputReading::new(
        &text(&rec["model"]),
        rec["context"].as_i64().expect("context"),
        in_float(&rec["decode_tokens_per_second"]),
        in_float(&rec["prefill_tokens_per_second"]),
    );
    reading.method = text(&rec["method"]);
    reading.provenance = text(&rec["provenance"]);
    reading
}

fn energy_in(rec: &Value) -> EnergyDeclaration {
    EnergyDeclaration {
        draw_watts: in_float(&rec["draw_watts"]),
        usd_per_kwh: in_float(&rec["usd_per_kwh"]),
        source: text(&rec["source"]),
        as_of: text(&rec["as_of"]),
        provenance: text(&rec["provenance"]),
    }
}

fn price_in(rec: &Value) -> Option<ProviderPrice> {
    if rec.is_null() {
        return None;
    }
    let mut price = ProviderPrice::new(&text(&rec["provider"]), &text(&rec["model"]));
    price.usd_per_million_input = in_float(&rec["usd_per_million_input"]);
    price.usd_per_million_output = in_float(&rec["usd_per_million_output"]);
    price.source = text(&rec["source"]);
    price.as_of = text(&rec["as_of"]);
    price.provenance = text(&rec["provenance"]);
    Some(price)
}

fn volume_in(rec: &Value) -> MonthlyVolume {
    MonthlyVolume {
        input_tokens: rec["input_tokens"].as_i64(),
        output_tokens: rec["output_tokens"].as_i64(),
        stated_by: text(&rec["stated_by"]),
    }
}

fn llamacpp_in(encoded: &Value) -> Option<LlamacppResponse> {
    if encoded.is_null() {
        return None;
    }
    Some(LlamacppResponse {
        model: (!encoded["model"].is_null()).then(|| meta_value(&encoded["model"])),
        // A list is a mapping; null (absent) and {"value": ...} (not a mapping) are not.
        timings: encoded["timings"].as_array().map(|items| {
            items
                .iter()
                .map(|pair| (text(&pair[0]), meta_value(&pair[1])))
                .collect()
        }),
    })
}

fn error_json(error: model_anatomy::Error) -> String {
    match error {
        model_anatomy::Error::InvalidInput(reason) => {
            serde_json::json!({ "error": reason }).to_string()
        }
        other => panic!("not a Python exception: {other}"),
    }
}

#[test]
fn every_economics_golden_matches_python() {
    check_all("economics", 70, |input| match input["kind"].as_str() {
        Some("throughput_from_llamacpp") => {
            let payload = llamacpp_in(&input["payload"]);
            let model = text(&input["model"]);
            let context = input["context"].as_i64().expect("context");
            match model_anatomy::throughput_from_llamacpp_json(payload.as_ref(), &model, context) {
                Ok(json) => {
                    let typed =
                        model_anatomy::throughput_from_llamacpp(payload.as_ref(), &model, context)
                            .expect("typed reading");
                    let doc: Value = serde_json::from_str(&json).expect("core JSON");
                    assert_eq!(doc["complete"].as_bool(), Some(typed.complete()));
                    json
                }
                Err(error) => error_json(error),
            }
        }
        Some("compare") => {
            let throughput = throughput_in(&input["throughput"]);
            let energy = energy_in(&input["energy"]);
            let price = price_in(&input["price"]);
            let volume = volume_in(&input["volume"]);
            let typed = model_anatomy::compare_costs(&throughput, &energy, price.as_ref(), &volume)
                .expect("typed comparison");
            assert_eq!(typed.established, typed.refusals.is_empty());
            assert_eq!(typed.inputs_complete.throughput, throughput.complete());
            model_anatomy::compare_costs_json(&throughput, &energy, price.as_ref(), &volume)
                .expect("comparison")
        }
        Some("usd_for_seconds") => {
            let energy = energy_in(&input["energy"]);
            let seconds = in_float(&input["seconds"]).expect("seconds");
            model_anatomy::usd_for_seconds(&energy, seconds).expect("typed usd");
            model_anatomy::usd_for_seconds_json(&energy, seconds).expect("usd")
        }
        Some("read_power") => {
            let meter = match input["meter"].as_str() {
                Some("none") => MeterOutcome::NoMeter,
                Some("raises") => MeterOutcome::of(|| Err::<Option<f64>, &str>("no such tool")),
                Some("returns") => {
                    let watts = in_float(&input["watts"]);
                    MeterOutcome::of(|| Ok::<_, ()>(watts))
                }
                other => panic!("meter {other:?}"),
            };
            model_anatomy::read_power(meter).expect("typed power");
            model_anatomy::read_power_json(meter).expect("power")
        }
        other => panic!("kind {other:?}"),
    });
}

#[test]
fn the_economics_constants_and_caveats_match_python() {
    let golden = read(&goldens().join("economics_constants.json"));
    let actual: Value =
        serde_json::from_str(&model_anatomy::economics_constants_json().expect("constants"))
            .expect("core JSON");
    assert_same(&actual, &golden, "economics constants");
    let caveats = model_anatomy::structural_caveats().expect("caveats");
    assert_eq!(caveats.len(), 4);
    // Every comparison, refused or not, carries them byte for byte.
    for path in files("economics") {
        let golden = read(&path);
        if golden["input"]["kind"] == "compare" {
            assert_eq!(texts(&golden["expected"]["caveats"]), caveats);
        }
    }
}

// ── PR 4: the morphometry comparison ────────────────────────────────────────

use model_anatomy::{CellPresence, ComparisonCode, MorphometryRecord, RecordCell};

fn record_of(encoded: &Value) -> MorphometryRecord {
    let order = texts(&encoded["native_axis_order"]);
    assert_eq!(order.len(), 2, "a two-axis order");
    MorphometryRecord {
        model: text(&encoded["model"]),
        source: text(&encoded["source"]),
        schema_version: text(&encoded["schema_version"]),
        refusal: text(&encoded["refusal"]),
        gaps: texts(&encoded["gaps"]),
        native_axis_order: [order[0].clone(), order[1].clone()],
        expert_count: opt_i64(&encoded["expert_count"]),
        expert_used_count: opt_i64(&encoded["expert_used_count"]),
        cells: encoded["cells"]
            .as_array()
            .expect("cells")
            .iter()
            .map(|cell| RecordCell {
                block: opt_i64(&cell["block"]),
                module: text(&cell["module"]),
                parameters: cell["parameters"].as_i64().expect("parameters"),
                tensors: cell["tensors"].as_i64().expect("tensors"),
                element_types: texts(&cell["element_types"]),
                nominal_bytes: opt_i64(&cell["nominal_bytes"]),
                routed_parameters: cell["routed_parameters"].as_i64().expect("routed"),
            })
            .collect(),
    }
}

/// The side's payload and model, when it was compared as `analyze` returned it
/// (an edited side carries only its record).
fn analyzed_side(side: &Value) -> Option<(Option<Payload>, String)> {
    side.get("payload")
        .map(|payload| (payload_of(payload), text(&side["model"])))
}

#[test]
fn every_compare_golden_matches_python() {
    // Every case through the record entry point, with the typed result's `ok`
    // and `complete` held to Python's properties.
    check_all("compare", 40, |input| {
        let left = record_of(&input["left"]["record"]);
        let right = record_of(&input["right"]["record"]);
        let json = model_anatomy::compare_records_json(&left, &right).expect("comparison");
        let typed = model_anatomy::compare_records(&left, &right).expect("typed comparison");
        let doc: Value = serde_json::from_str(&json).expect("core JSON");
        assert_eq!(doc["ok"].as_bool(), Some(typed.ok()));
        assert_eq!(doc["complete"].as_bool(), Some(typed.complete()));
        assert_eq!(doc["code"].as_str(), Some(typed.code.as_str()));
        json
    });
}

#[test]
fn every_compare_golden_from_payloads_matches_python() {
    // A case whose sides are both `analyze` results also goes through
    // `ma_compare` (the C++ analyses them), and each analysed record equals the
    // one Python's `compare` read.
    check_all("compare", 40, |input| {
        let records = [
            record_of(&input["left"]["record"]),
            record_of(&input["right"]["record"]),
        ];
        let sides = [
            analyzed_side(&input["left"]),
            analyzed_side(&input["right"]),
        ];
        for (side, record) in sides.iter().zip(&records) {
            if let Some((payload, model)) = side {
                let analysis = model_anatomy::analyze(payload.as_ref(), model).expect("analysis");
                assert_eq!(&MorphometryRecord::from(&analysis.morphometry), record);
            }
        }
        match sides {
            [Some((left, left_model)), Some((right, right_model))] => {
                model_anatomy::compare(left.as_ref(), &left_model, right.as_ref(), &right_model)
                    .expect("typed comparison");
                model_anatomy::compare_json(
                    left.as_ref(),
                    &left_model,
                    right.as_ref(),
                    &right_model,
                )
                .expect("comparison")
            }
            // An edited side has no payload; the record path answers for it.
            _ => model_anatomy::compare_records_json(&records[0], &records[1]).expect("comparison"),
        }
    });
}

#[test]
fn the_compare_goldens_reach_every_code_and_presence() {
    let mut codes = std::collections::HashSet::new();
    let mut presences = std::collections::HashSet::new();
    let mut complete = 0;
    let mut from_payloads = 0;
    for path in files("compare") {
        let golden = read(&path);
        let expected = &golden["expected"];
        let code: ComparisonCode =
            serde_json::from_value(expected["code"].clone()).expect("a comparison code");
        codes.insert(code);
        for cell in expected["cells"].as_array().expect("cells") {
            let presence: CellPresence =
                serde_json::from_value(cell["presence"].clone()).expect("a presence");
            presences.insert(presence);
        }
        complete += usize::from(expected["complete"].as_bool() == Some(true));
        let input = &golden["input"];
        if input["left"].get("payload").is_some() && input["right"].get("payload").is_some() {
            from_payloads += 1;
        }
    }
    assert_eq!(codes.len(), 6, "every ComparisonCode: {codes:?}");
    assert_eq!(presences.len(), 3, "every CellPresence: {presences:?}");
    assert!(complete >= 1, "a complete comparison");
    assert!(
        from_payloads >= 15,
        "only {from_payloads} cases from payloads"
    );
}

// ── PR 6: dequantisation and weight statistics ───────────────────────────────

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn le_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_bits().to_le_bytes()).collect()
}

#[test]
fn every_dequant_golden_matches_gguf_py_bit_for_bit() {
    let mut seen = std::collections::BTreeSet::new();
    for path in files("dequant") {
        let golden = read(&path);
        let name = text(&golden["type"]);
        let type_id = golden["type_id"].as_u64().expect("type_id") as u32;
        let input = unhex(golden["input_hex"].as_str().expect("input_hex"));
        let values =
            model_anatomy::dequantize(type_id, &input).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            values.len() as u64,
            golden["elements"].as_u64().unwrap(),
            "{name}: element count"
        );
        for sample in golden["samples"].as_array().expect("samples") {
            let index = sample[0].as_u64().unwrap() as usize;
            let bits =
                u32::from_str_radix(sample[1].as_str().unwrap().trim_start_matches("0x"), 16)
                    .unwrap();
            assert_eq!(
                values[index].to_bits(),
                bits,
                "{name}: element {index} is {:#010x}, gguf-py's {bits:#010x}",
                values[index].to_bits()
            );
        }
        let digest = model_anatomy::sha256_hex(&le_bytes(&values)).unwrap();
        assert_eq!(
            digest,
            text(&golden["sha256"]),
            "{name}: SHA-256 of the float32 output"
        );
        seen.insert(name);
    }
    let supported: std::collections::BTreeSet<String> = model_anatomy::quant_types()
        .unwrap()
        .into_iter()
        .filter(|t| t.supported)
        .map(|t| t.name)
        .collect();
    assert_eq!(seen, supported, "every supported type has a golden, and only those");
    assert_eq!(seen.len(), 22);
}

/// The tolerance on the float sums and the figures derived from them (README,
/// PR 6, D16): counts, zeros, the histogram and the extremes exactly; each sum
/// within 1e-12 of its scale.
const SUM_TOLERANCE: f64 = 1e-12;

fn within(actual: f64, expected: f64, scale: f64) -> bool {
    (actual - expected).abs() <= SUM_TOLERANCE * scale.abs().max(f64::MIN_POSITIVE)
}

fn check_moments(actual: &model_anatomy::WeightMoments, expected: &Value, what: &str) {
    let int = |key: &str| {
        expected[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{what}: {key}"))
    };
    let float = |key: &str| expected[key].as_f64();
    assert_eq!(actual.count, int("count"), "{what}: count");
    assert_eq!(actual.non_finite, int("non_finite"), "{what}: non_finite");
    assert_eq!(actual.zeros, int("zeros"), "{what}: zeros");
    let histogram: Vec<u64> = expected["histogram"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(actual.histogram, histogram, "{what}: histogram");
    // Extremes by value: the sign of a zero extreme is not specified.
    assert_eq!(actual.min, float("min"), "{what}: min");
    assert_eq!(actual.max, float("max"), "{what}: max");
    let sum_abs = float("sum_abs").unwrap();
    let sum_sq = float("sum_sq").unwrap();
    let sums = [
        ("sum", actual.sum, float("sum").unwrap(), sum_abs),
        ("sum_abs", actual.sum_abs, sum_abs, sum_abs),
        ("sum_sq", actual.sum_sq, sum_sq, sum_sq),
    ];
    for (key, a, e, scale) in sums {
        assert!(within(a, e, scale), "{what}: {key} {a:e} against {e:e}");
    }
    let derived = [
        ("mean", actual.mean, float("mean"), float("mean_abs")),
        ("mean_abs", actual.mean_abs, float("mean_abs"), float("mean_abs")),
        ("rms", actual.rms, float("rms"), float("rms")),
        ("l2", actual.l2, float("l2"), float("l2")),
    ];
    for (key, a, e, scale) in derived {
        assert_eq!(a.is_some(), e.is_some(), "{what}: {key} presence");
        if let (Some(a), Some(e), Some(scale)) = (a, e, scale) {
            assert!(within(a, e, scale), "{what}: {key} {a:e} against {e:e}");
        }
    }
    // std is the root of a difference of two near quantities: held on its
    // square, at the scale of rms^2.
    assert_eq!(
        actual.std.is_some(),
        float("std").is_some(),
        "{what}: std presence"
    );
    if let (Some(a), Some(e), Some(rms)) = (actual.std, float("std"), float("rms")) {
        assert!(
            within(a * a, e * e, rms * rms),
            "{what}: std {a:e} against {e:e}"
        );
    }
}

fn weights_file_bytes(name: &str, input: &Value) -> Vec<u8> {
    match input["gguf_hex"].as_str() {
        Some(hex) => unhex(hex),
        None => {
            let base = read(
                &goldens()
                    .join("weights")
                    .join(format!("{}.json", text(&input["base"]))),
            );
            let mut bytes = unhex(base["input"]["gguf_hex"].as_str().expect("the base's bytes"));
            let cut = input["truncate_by"].as_u64().unwrap() as usize;
            assert!(cut < bytes.len(), "{name}: cut");
            bytes.truncate(bytes.len() - cut);
            bytes
        }
    }
}

fn check_weights(actual: &model_anatomy::WeightStatistics, expected: &Value, what: &str) {
    assert_eq!(actual.refusal, None, "{what}: refusal");
    assert_eq!(
        actual.gguf_version.map(u64::from),
        expected["gguf_version"].as_u64(),
        "{what}: version"
    );
    assert_eq!(actual.alignment, expected["alignment"].as_u64(), "{what}: alignment");
    assert_eq!(
        actual.file_bytes,
        expected["file_bytes"].as_u64().unwrap(),
        "{what}: file_bytes"
    );
    assert_eq!(
        actual.tensor_count,
        expected["tensor_count"].as_u64(),
        "{what}: tensor_count"
    );
    assert_eq!(
        actual.bytes_total,
        expected["bytes_total"].as_u64(),
        "{what}: bytes_total"
    );
    assert_eq!(
        actual.complete,
        expected["complete"].as_bool().unwrap(),
        "{what}: complete"
    );
    let tensors = expected["tensors"].as_array().unwrap();
    assert_eq!(actual.tensors.len(), tensors.len(), "{what}: tensor count");
    for (a, e) in actual.tensors.iter().zip(tensors) {
        let at = format!("{what}: {}", a.name);
        assert_eq!(a.name, text(&e["name"]), "{at}");
        assert_eq!(a.type_name, text(&e["type"]), "{at}: type");
        assert_eq!(
            u64::from(a.type_id),
            e["type_id"].as_u64().unwrap(),
            "{at}: type_id"
        );
        let dims: Vec<u64> = e["dims"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d.as_u64().unwrap())
            .collect();
        assert_eq!(a.dims, dims, "{at}: dims");
        assert_eq!(a.offset, e["offset"].as_u64().unwrap(), "{at}: offset");
        assert_eq!(a.bytes, e["bytes"].as_u64().unwrap(), "{at}: bytes");
        assert_eq!(a.block, e["block"].as_i64(), "{at}: block");
        assert_eq!(a.module, text(&e["module"]), "{at}: module");
        check_moments(&a.stats, &e["stats"], &at);
    }
    let cells = expected["cells"].as_array().unwrap();
    assert_eq!(actual.cells.len(), cells.len(), "{what}: cell count");
    for (a, e) in actual.cells.iter().zip(cells) {
        let at = format!("{what}: cell ({:?}, {})", a.block, a.module);
        assert_eq!(a.block, e["block"].as_i64(), "{at}");
        assert_eq!(a.module, text(&e["module"]), "{at}");
        assert_eq!(a.tensors, e["tensors"].as_u64().unwrap(), "{at}: tensors");
        check_moments(&a.stats, &e["stats"], &at);
    }
    let refused = expected["refused"].as_array().unwrap();
    assert_eq!(actual.refused.len(), refused.len(), "{what}: refused count");
    for (a, e) in actual.refused.iter().zip(refused) {
        let at = format!("{what}: refused {}", a.name);
        assert_eq!(a.name, text(&e["name"]), "{at}");
        assert_eq!(a.type_name, text(&e["type"]), "{at}: type");
        assert_eq!(
            u64::from(a.type_id),
            e["type_id"].as_u64().unwrap(),
            "{at}: type_id"
        );
        assert_eq!(a.block, e["block"].as_i64(), "{at}: block");
        assert_eq!(a.module, text(&e["module"]), "{at}: module");
        assert_eq!(a.code, text(&e["code"]), "{at}: code");
    }
}

#[test]
fn every_weights_golden_matches_python_at_every_thread_count() {
    let mut codes = std::collections::BTreeSet::new();
    let mut count = 0;
    for path in files("weights") {
        let golden = read(&path);
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let bytes = weights_file_bytes(&name, &golden["input"]);
        let file = std::env::temp_dir().join(format!(
            "model_anatomy_parity_{}_{name}.gguf",
            std::process::id()
        ));
        fs::write(&file, &bytes).unwrap();
        let one = model_anatomy::weight_statistics_with_threads(&file, 1, None, None);
        let many = model_anatomy::weight_statistics_with_threads(&file, 0, None, None);
        let _ = fs::remove_file(&file);
        let one = one.unwrap();
        let mut many = many.unwrap();
        check_weights(&one, &golden["expected"], &name);
        // The figures do not depend on how many workers there were.
        assert_eq!(one.threads, Some(1));
        many.threads = Some(1);
        many.path = one.path.clone();
        assert_eq!(one, many, "{name}: one worker against the default");
        codes.extend(one.refused.iter().map(|r| r.code.clone()));
        count += 1;
    }
    assert!(count >= 3, "only {count} weights goldens");
    assert!(
        codes.contains("UNSUPPORTED_TYPE") && codes.contains("TRUNCATED"),
        "{codes:?}"
    );
}
