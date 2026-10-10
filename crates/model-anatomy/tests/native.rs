//! What the goldens cannot pin: the SHA-256 against published vectors, and each
//! deliberate deviation from Python (README, DEVIATIONS), where Python's answer
//! would be the wrong expectation.

use model_anatomy::{MetaValue, Payload, PayloadTensor, analyze, sha256_hex};

fn tensor(name: &str, type_name: &str, shape: &[u64]) -> PayloadTensor {
    PayloadTensor {
        name: name.to_owned(),
        shape: shape.to_vec(),
        type_name: type_name.to_owned(),
    }
}

fn payload(info: &[(&str, MetaValue)], tensors: Option<Vec<PayloadTensor>>) -> Payload {
    Payload {
        model_info: info
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        tensors,
        ..Payload::default()
    }
}

fn int(value: i64) -> MetaValue {
    MetaValue::Int(value.to_string())
}

#[test]
fn sha256_matches_the_fips_180_examples_and_padding_boundaries() {
    // FIPS 180-2 Appendix B (one block, two blocks, a million 'a').
    assert_eq!(
        sha256_hex(b"").unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc").unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").unwrap(),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        sha256_hex(
            b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
        )
        .unwrap(),
        "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
    );
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]).unwrap(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    // Lengths either side of the padding boundaries (Python's hashlib, 2026-10-07).
    for (length, expected) in [
        (
            55,
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
        ),
        (
            56,
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
        ),
        (
            63,
            "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
        ),
        (
            64,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
        ),
        (
            65,
            "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
        ),
        (
            119,
            "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb",
        ),
        (
            120,
            "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c",
        ),
    ] {
        assert_eq!(
            sha256_hex(&vec![b'a'; length]).unwrap(),
            expected,
            "{length} bytes"
        );
    }
}

fn refused_out_of_range(payload: &Payload, mentions: &str) {
    let analysis = analyze(Some(payload), "m").expect("analysis");
    let morph = &analysis.morphometry;
    assert_eq!(morph.refusal, "INTEGER_OUT_OF_RANGE");
    assert_eq!(morph.model, "m");
    assert!(morph.cells.is_empty() && morph.tensors.is_empty());
    assert!(!morph.derived.ok);
    assert_eq!(morph.gaps.len(), 1);
    assert!(morph.gaps[0].contains("deviation D2"), "{}", morph.gaps[0]);
    assert!(morph.gaps[0].contains(mentions), "{}", morph.gaps[0]);
    assert_eq!(analysis.voxel_field.occupied, 0);
}

#[test]
fn d2_an_integer_past_int64_is_a_named_refusal_not_a_wrapped_figure() {
    // Python holds each of these exactly; this port refuses rather than wrap.
    let big = MetaValue::Int("9223372036854775808".into()); // 2^63
    refused_out_of_range(
        &payload(&[("general.parameter_count", big)], None),
        "general.parameter_count",
    );
    refused_out_of_range(
        &payload(
            &[
                ("general.architecture", MetaValue::Str("q".into())),
                ("q.block_count", MetaValue::F64(1e19)),
            ],
            None,
        ),
        "q.block_count",
    );
    refused_out_of_range(
        &payload(
            &[(
                "x.expert_count",
                MetaValue::Str(" 99999999999999999999 ".into()),
            )],
            Some(vec![]),
        ),
        "x.expert_count",
    );
    refused_out_of_range(
        &payload(&[], Some(vec![tensor("a", "F32", &[1 << 63])])),
        "a dimension of tensor a",
    );
    refused_out_of_range(
        &payload(&[], Some(vec![tensor("a", "F32", &[1 << 32, 1 << 32])])),
        "a tensor's parameter count",
    );
    // Each fits; their sum does not.
    refused_out_of_range(
        &payload(
            &[],
            Some(vec![
                tensor("token_embd.weight", "", &[1 << 62]),
                tensor("output.weight", "", &[1 << 62]),
            ]),
        ),
        "the counted parameter total",
    );
    // The stored count fits; its nominal bytes (x4 for F32) do not.
    refused_out_of_range(
        &payload(&[], Some(vec![tensor("a", "F32", &[1 << 62])])),
        "nominal bytes",
    );
    // INT64_MIN itself fits and is carried.
    let min = analyze(
        Some(&payload(
            &[(
                "general.parameter_count",
                MetaValue::Int(i64::MIN.to_string()),
            )],
            None,
        )),
        "m",
    )
    .unwrap();
    assert_eq!(min.morphometry.declared_parameters, Some(i64::MIN));
    assert_eq!(min.morphometry.refusal, "BLOCK_COUNT_NOT_DECLARED");
}

#[test]
fn d2_a_zero_dimension_drops_the_tensor_before_a_huge_one_can_refuse() {
    // Python stops at the zero and drops the tensor; so does the port.
    let analysis = analyze(
        Some(&payload(&[], Some(vec![tensor("a", "F32", &[1 << 63, 0])]))),
        "m",
    )
    .unwrap();
    assert_eq!(analysis.morphometry.refusal, "BLOCK_COUNT_NOT_DECLARED");
    assert!(analysis.morphometry.gaps[0].contains("1 tensor entries had no usable name or shape"));
}

#[test]
fn d3_text_rules_are_ascii_only() {
    // U+212A KELVIN SIGN lowercases to ASCII 'k' in Python ("attn_k");
    // ASCII-only lowering leaves it, so the tensor is Unclassified here.
    // Arabic-Indic digits match Python's \d (block 3); not here (no block).
    // U+00A0 is stripped by Python's str.strip(); not here.
    let analysis = analyze(
        Some(&payload(
            &[
                ("general.architecture", MetaValue::Str("q".into())),
                ("q.block_count", MetaValue::Str("\u{a0}2".into())),
            ],
            Some(vec![
                tensor("blk.0.attn_\u{212a}.weight", "F32", &[2]),
                tensor("blk.\u{663}.attn_q.weight", "F32", &[2]),
            ]),
        )),
        "m",
    )
    .unwrap();
    let morph = &analysis.morphometry;
    assert_eq!(morph.tensors[0].module, "other");
    assert_eq!(morph.tensors[1].block, None);
    assert_eq!(
        morph.block_count, None,
        "\\u00a02 is not an integer under ASCII rules"
    );
    // Non-ASCII text passes through unchanged and round-trips byte for byte.
    assert_eq!(morph.tensors[0].name, "blk.0.attn_\u{212a}.weight");
}

fn declared(blocks: i64) -> Payload {
    payload(
        &[
            ("general.architecture", MetaValue::Str("llama".into())),
            ("llama.block_count", int(blocks)),
            ("llama.embedding_length", int(2)),
            ("llama.feed_forward_length", int(2)),
            ("llama.attention.head_count", int(1)),
        ],
        None,
    )
}

#[test]
fn d4_the_declared_architecture_path_shares_the_block_budget() {
    let at_budget = analyze(Some(&declared(4096)), "m").unwrap();
    assert!(at_budget.morphometry.derived.ok);
    assert_eq!(at_budget.morphometry.cells.len(), 1 + 4096 * 9);
    assert_eq!(at_budget.voxel_field.width, 4096);

    let past = analyze(Some(&declared(4097)), "m").unwrap();
    assert_eq!(past.morphometry.refusal, "BLOCK_COUNT_NOT_DECLARED");
    assert!(past.morphometry.cells.is_empty());
    assert!(
        past.morphometry
            .gaps
            .last()
            .unwrap()
            .contains("deviation D4")
    );

    // A declaration no machine could materialise is refused, not attempted.
    let absurd = analyze(Some(&declared(i64::MAX)), "m").unwrap();
    assert_eq!(absurd.morphometry.refusal, "BLOCK_COUNT_NOT_DECLARED");
}

#[test]
fn the_inventory_block_budget_boundary_is_measured_then_refused() {
    let blocks = |count: u64| {
        payload(
            &[],
            Some(
                (0..count)
                    .map(|b| tensor(&format!("blk.{b}.attn_q.weight"), "F16", &[2]))
                    .collect(),
            ),
        )
    };
    let at = analyze(Some(&blocks(4096)), "m").unwrap();
    assert!(at.morphometry.derived.ok);
    assert_eq!(at.morphometry.derived.blocks.len(), 4096);
    assert_eq!(at.voxel_field.capacity, 4096 * 5 * 5);
    let past = analyze(Some(&blocks(4097)), "m").unwrap();
    assert_eq!(past.morphometry.refusal, "BLOCK_COUNT_NOT_DECLARED");
}

#[test]
fn identical_input_gives_identical_bytes() {
    let p = declared(3);
    let a = model_anatomy::analyze_json(Some(&p), "m").unwrap();
    let b = model_anatomy::analyze_json(Some(&p), "m").unwrap();
    assert_eq!(a, b);
}

// ── PR 2: registration, the template hierarchy (D6-D8) ──────────────────────

use model_anatomy::{CoordinateAxis, CoordinateSpace, Error, TemplateNode, TemplateRef};

fn ordinal(axis_id: &str, ordering: &str) -> CoordinateAxis {
    let mut axis = CoordinateAxis::new(
        axis_id,
        &format!("test:{axis_id}"),
        "DISCRETE_ORDINAL",
        "INTEGER",
        ordering,
    );
    axis.transform_policy = vec![
        "AXIS_ORDERING".into(),
        "AXIS_PERMUTATION".into(),
        "IDENTITY".into(),
    ];
    axis
}

fn space(axes: Vec<CoordinateAxis>) -> CoordinateSpace {
    CoordinateSpace {
        space_id: "test.space".into(),
        version: "1".into(),
        topology: "RECTANGULAR_TABLE".into(),
        axes,
        index_convention: "ZERO_BASED".into(),
        unit_system: None,
        reference_frame: None,
        valid_domain_rule: "ALL_DECLARED_COORDINATES".into(),
        region_atlas_refs: vec![],
        metadata: vec![],
    }
}

#[test]
fn d7_differing_axis_semantics_are_not_ported_and_say_so() {
    // Python answers EXACT_AXIS_ORDERING here (its ordering rung needs no
    // declaration); the port does not include that rung and must not guess.
    let report = model_anatomy::register(
        &space(vec![ordinal("a", "DESCENDING")]),
        &space(vec![ordinal("a", "ASCENDING")]),
    )
    .unwrap();
    assert_eq!(report.code, "NOT_PORTED");
    assert!(!report.compatible && report.transform.is_none());
    assert_eq!(
        report.failing_constraint.as_deref(),
        Some("registration_rung_not_ported")
    );
}

#[test]
fn d6_a_space_the_contract_could_not_hold_is_invalid_input_not_a_report() {
    let good = space(vec![ordinal("a", "ASCENDING")]);
    let mut bad = good.clone();
    bad.topology = "SPIRAL".into();
    assert!(matches!(
        model_anatomy::register(&bad, &good),
        Err(Error::InvalidInput(_))
    ));
    let wide = space(
        (0..65)
            .map(|i| ordinal(&format!("a{i}"), "ASCENDING"))
            .collect(),
    );
    assert!(matches!(
        model_anatomy::register(&wide, &wide),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn registration_is_unchanged_by_policy_order_as_the_contract_sorts_it() {
    let mut reversed = ordinal("a", "ASCENDING");
    reversed.transform_policy.reverse();
    let report = model_anatomy::register(
        &space(vec![reversed]),
        &space(vec![ordinal("a", "ASCENDING")]),
    )
    .unwrap();
    assert_eq!(report.code, "EXACT_IDENTITY");
}

fn node(id: &str, tier: &str, label: &str) -> TemplateNode {
    TemplateNode {
        template_ref: TemplateRef {
            template_id: id.into(),
            version: "1".into(),
        },
        tier: tier.into(),
        label: label.into(),
        description: "D".into(),
        parent_refs: vec![],
        coordinate_space: None,
        transform_policy: vec![],
        gaps: vec![],
    }
}

#[test]
fn d8_template_text_is_not_nfc_normalised() {
    // "e" + COMBINING ACUTE ACCENT: Python's NFC makes it one code point (U+00E9).
    let snapshot =
        model_anatomy::template_hierarchy(Some(&[node("root", "BASE_CONTRACT", "Cafe\u{301}")]))
            .unwrap();
    assert_eq!(snapshot.nodes[0].label, "Cafe\u{301}");
}

#[test]
fn a_node_python_would_not_construct_is_invalid_input_with_pythons_reason() {
    for (bad, reason) in [
        (
            node("root", "BASE_CONTRACT", "   "),
            "label must not be empty",
        ),
        (
            node("root", "NOT_A_TIER", "L"),
            "tier must be a TemplateTier",
        ),
        (
            node("root", "BASE_CONTRACT", "a\u{7}b"),
            "label must not contain control characters",
        ),
    ] {
        match model_anatomy::template_hierarchy(Some(&[bad])) {
            Err(Error::InvalidInput(why)) => assert!(why.ends_with(reason), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn the_lineage_draws_the_models_own_volume_unchanged_and_builds_it_once() {
    let payload = payload(
        &[("general.architecture", MetaValue::Str("qwen3".into()))],
        Some(vec![
            tensor("blk.0.attn_q.weight", "F16", &[8, 8]),
            tensor("blk.1.attn_q.weight", "F16", &[8, 8]),
        ]),
    );
    let layers = model_anatomy::measure_layers(Some((Some(&payload), "m")), None).unwrap();
    assert!(layers.model_measured);
    assert_eq!(layers.volumes_built, 1);
    let drawn: Vec<_> = layers.layers.iter().filter(|l| l.drawn).collect();
    assert_eq!(drawn.len(), 1);
    let own = analyze(Some(&payload), "m").unwrap().voxel_field;
    assert_eq!(drawn[0].occupied, own.occupied);
    assert_eq!(layers.field.as_ref(), Some(&own));
}

// ── PR 3: the Foundry's deviations (D9-D12) and R1 ───────────────────────────

use model_anatomy::{FoundryModel, Gpu, Reading};

fn llama(params: i64, extra: &[(&str, MetaValue)]) -> Payload {
    let mut info = vec![
        ("general.architecture", MetaValue::Str("llama".into())),
        ("general.parameter_count", int(params)),
        ("llama.block_count", int(32)),
        ("llama.embedding_length", int(4096)),
        ("llama.feed_forward_length", int(14336)),
        ("llama.attention.head_count", int(32)),
        ("llama.attention.head_count_kv", int(8)),
        ("llama.context_length", int(8192)),
    ];
    info.extend(extra.iter().cloned());
    Payload {
        quantization_level: "Q4_K_M".into(),
        ..payload(&info, None)
    }
}

fn rx(backend: &str, probe: &str) -> Reading {
    Reading {
        probe: probe.into(),
        backend: backend.into(),
        gpu: Some(Gpu {
            name: "AMD Radeon RX 9070 XT".into(),
            total_bytes: Some(17_095_983_104),
            free_bytes: None,
            vendor_id: Some(0x1002),
            device_id: Some(0x7550),
        }),
        ram_total_bytes: Some(64 << 30),
        ..Reading::default()
    }
}

fn one(payload: Payload) -> Vec<FoundryModel> {
    vec![FoundryModel {
        name: "m".into(),
        payload: Some(payload),
        ..FoundryModel::default()
    }]
}

#[test]
fn r1_the_reserve_is_read_from_alelyon_hf_mem_fraction_as_python_reads_it() {
    let reading = rx("rocm", "");
    let at = |fraction: Option<&str>| {
        model_anatomy::foundry(
            &reading,
            &one(llama(8_030_261_248, &[])),
            8192,
            fraction,
            "",
            "",
        )
        .unwrap()
    };
    // Unset: local_hf's default cap 0.85, so the reserve is 1.0 - 0.85 as a double.
    let unset = at(None);
    assert_eq!(unset.reserve.fraction.to_bits(), (1.0f64 - 0.85).to_bits());
    assert_eq!(unset.reserve.fraction, 0.15000000000000002);
    assert_eq!(unset.reserve.provenance, "OBSERVED");
    assert!(
        unset.native.reserve_basis.contains("unset"),
        "{}",
        unset.native.reserve_basis
    );
    // int(17_095_983_104 * (1.0 - 0.15000000000000002)).
    let budget = unset.catalog[0]
        .verdict
        .as_ref()
        .unwrap()
        .budget_bytes
        .unwrap();
    assert_eq!(budget, (17_095_983_104f64 * (1.0 - (1.0 - 0.85))) as i64);
    let refused = at(Some("abc"));
    assert_eq!(
        (
            refused.reserve.fraction,
            refused.reserve.provenance.as_str()
        ),
        (0.15, "DECLARED")
    );
    assert!(refused.native.reserve_basis.contains("fallback"));
}

#[test]
fn d9_an_integer_past_int64_refuses_the_shape_not_a_wrapped_figure() {
    let huge = MetaValue::Int("99999999999999999999".into());
    let shape =
        model_anatomy::shape(Some(&llama(1, &[("llama.context_length", huge)])), "m").unwrap();
    assert_eq!(shape.native.refusal, "INTEGER_OUT_OF_RANGE");
    assert_eq!(shape.model, "m");
    assert!(shape.layers.is_none() && shape.weight_bytes.is_none());
    assert!(shape.kv_bytes_per_token.is_none());
    assert_eq!(shape.weight_provenance, "UNMEASURED");
    assert_eq!(shape.gaps.len(), 1);
    assert!(shape.gaps[0].contains("deviation D9"), "{}", shape.gaps[0]);
    // The bytes one token adds, past int64: Python's product is exact, this refuses.
    let wide = llama(
        1,
        &[
            ("llama.block_count", int(1 << 40)),
            ("llama.attention.head_count_kv", int(1 << 40)),
        ],
    );
    let shape = model_anatomy::shape(Some(&wide), "m").unwrap();
    assert_eq!(shape.native.refusal, "INTEGER_OUT_OF_RANGE");
    assert!(
        shape.gaps[0].contains("KV cache bytes per token"),
        "{}",
        shape.gaps[0]
    );
    // A refused shape is UNDECIDABLE in the Foundry, never a verdict.
    let foundry = model_anatomy::foundry(&rx("rocm", ""), &one(wide), 4096, None, "", "").unwrap();
    assert_eq!(foundry.runnable[0].fit.verdict, "UNDECIDABLE");
    assert!(foundry.native.refusal.is_empty());
}

#[test]
fn d9_a_cache_past_int64_at_the_chosen_context_refuses_the_whole_reading() {
    // 2^20 layers x 2^20 kv heads x (2^18 + 2^18): 2^59 bytes a token, which fits;
    // at 131,072 tokens and two bytes an element it does not.
    let deep = llama(
        1,
        &[
            ("llama.block_count", int(1 << 20)),
            ("llama.attention.head_count_kv", int(1 << 20)),
            ("llama.attention.key_length", int(1 << 18)),
            ("llama.attention.value_length", int(1 << 18)),
        ],
    );
    assert_eq!(
        model_anatomy::shape(Some(&deep), "m")
            .unwrap()
            .kv_bytes_per_token,
        Some(1 << 59)
    );
    let foundry =
        model_anatomy::foundry(&rx("rocm", ""), &one(deep), 131_072, None, "", "").unwrap();
    assert_eq!(foundry.native.refusal, "INTEGER_OUT_OF_RANGE");
    assert!(
        foundry.native.gaps[0].contains("deviation D9"),
        "{:?}",
        foundry.native.gaps
    );
    assert!(foundry.catalog.is_empty() && foundry.runnable.is_empty() && foundry.pairs.is_empty());
}

#[test]
fn d10_the_native_backend_is_vulkan_and_counts_as_an_accelerator() {
    let reading = rx("vulkan", "dxgi");
    assert!(reading.has_accelerator());
    let foundry = model_anatomy::foundry(
        &reading,
        &one(llama(8_030_261_248, &[])),
        8192,
        None,
        "",
        "",
    )
    .unwrap();
    let fit = &foundry.runnable[0].fit;
    // The accelerator budget (85% of the RX), not the CPU's 65% of system RAM.
    assert_eq!(fit.verdict, "RUNS");
    assert!(
        fit.budget_basis.starts_with("85% of accelerator memory"),
        "{}",
        fit.budget_basis
    );
    assert_eq!(fit.budget_provenance, "OBSERVED");
    // The probe named the backend, so its row is OBSERVED with no torch version.
    let rows = model_anatomy::describe(&reading).unwrap().rows;
    assert_eq!(
        rows[0],
        ("Compute backend".into(), "vulkan".into(), "OBSERVED".into())
    );
    // Python's rule without the probe's name: no torch version, so UNMEASURED.
    let rows = model_anatomy::describe(&rx("vulkan", "")).unwrap().rows;
    assert_eq!(rows[0].2, "UNMEASURED");
    assert_eq!(
        rows[3],
        (
            "Accelerator memory free now".into(),
            "UNMEASURED".into(),
            "UNMEASURED".into()
        )
    );
}

#[test]
fn d10_the_probe_never_falls_back_to_the_first_adapter() {
    for (filter, says) in [
        (None, "VK_LOADER_DEVICE_ID_FILTER is unset"),
        (Some("zz-not-hex"), "not one hexadecimal device id"),
        (
            Some("0xfffe"),
            "no adapter DXGI enumerated has device id 0xfffe",
        ),
    ] {
        let reading = model_anatomy::probe_with(filter, None).unwrap();
        assert_eq!(reading.probe, "dxgi");
        assert_eq!(reading.backend, "absent", "{filter:?}");
        assert!(reading.gpu.is_none(), "{filter:?}: an adapter was chosen");
        assert!(
            reading.gaps.iter().any(|g| g.contains(says)),
            "{filter:?}: {:?}",
            reading.gaps
        );
        assert!(reading.bf16.is_none() && reading.disk_free_bytes.is_none());
        assert!(reading.gaps.iter().any(|g| g.contains("never does")));
        // With no adapter chosen, accelerator memory reads UNMEASURED.
        let rows = model_anatomy::describe(&reading).unwrap().rows;
        assert_eq!(
            rows[2],
            (
                "Accelerator memory".into(),
                "UNMEASURED".into(),
                "UNMEASURED".into()
            )
        );
    }
}

#[test]
#[ignore = "reads this machine (no device opened): run by hand with VK_LOADER_DEVICE_ID_FILTER set"]
fn the_probe_reads_this_machine() {
    let reading = model_anatomy::probe().unwrap();
    let json = model_anatomy::probe_json_with(
        reading.device_filter.as_deref(),
        reading.disk_root.as_deref(),
    )
    .unwrap();
    let pretty: serde_json::Value = serde_json::from_str(&json).unwrap();
    println!("{}", serde_json::to_string_pretty(&pretty).unwrap());
    for (label, value, provenance) in model_anatomy::describe(&reading).unwrap().rows {
        println!("{label}: {value} [{provenance}]");
    }
    if let Some(filter) = &reading.device_filter {
        let gpu = reading
            .gpu
            .as_ref()
            .expect("the filtered adapter is chosen");
        let wanted = u32::from_str_radix(filter.trim().trim_start_matches("0x"), 16).unwrap();
        assert_eq!(gpu.device_id, Some(wanted));
        assert_eq!(reading.backend, "vulkan");
        // Free memory is read from the adapter's own counter: present, and no more than the adapter holds.
        let free = gpu.free_bytes.expect("the GPU Adapter Memory counter read");
        assert!(free >= 0 && Some(free) <= gpu.total_bytes, "{free} of {:?}", gpu.total_bytes);
        let rows = model_anatomy::describe(&reading).unwrap().rows;
        assert_eq!(rows[3].0, "Accelerator memory free now");
        assert_eq!(rows[3].2, "OBSERVED");
    }
}

#[test]
fn d11_no_margin_is_ever_declared_and_the_cache_is_sixteen_bit() {
    let foundry = model_anatomy::foundry(
        &rx("rocm", ""),
        &one(llama(8_030_261_248, &[])),
        4096,
        None,
        "",
        "",
    )
    .unwrap();
    let footprint = &foundry.runnable[0].fit.footprint;
    assert!(footprint.margin.is_none());
    assert_eq!(footprint.kv_precision, "f16");
    // 32 layers x 4,096 tokens x 8 kv heads x (128 + 128) x 2 bytes.
    assert_eq!(footprint.kv_cache.bytes, Some(32 * 4096 * 8 * 256 * 2));
    assert!(
        footprint.kv_cache.rule.ends_with("x 2.0B"),
        "{}",
        footprint.kv_cache.rule
    );
}

#[test]
fn d12_the_cap_is_read_per_call_and_its_text_rules_are_ascii() {
    let reading = rx("rocm", "");
    let fit = |fraction: Option<&str>| {
        model_anatomy::foundry(&reading, &one(llama(1, &[])), 8192, fraction, "", "").unwrap()
    };
    // Python reads the variable once, when local_hf is first imported; here each
    // reading takes the text it is given.
    let budget = |fraction| fit(Some(fraction)).runnable[0].fit.budget_bytes.unwrap();
    assert!(budget("0.5") < budget("0.9"));
    // float() strips Unicode spaces (U+00A0 here); this port's text rules are
    // ASCII (D3), so the cap is refused and the declared fallback is used.
    let f = fit(Some("\u{a0}0.5"));
    assert_eq!(
        (f.reserve.fraction, f.reserve.provenance.as_str()),
        (0.15, "DECLARED")
    );
    let f = fit(Some(" 0.5\t"));
    assert_eq!(
        (f.reserve.fraction, f.reserve.provenance.as_str()),
        (0.5, "OBSERVED")
    );
}

// ── PR 4: the morphometry comparison ────────────────────────────────────────

use model_anatomy::{
    CellPresence, ComparisonCode, MorphometryRecord, Ratio, RecordCell, compare, compare_records,
    compare_records_json,
};

fn cell(block: Option<i64>, module: &str, parameters: i64, routed: i64) -> RecordCell {
    RecordCell {
        block,
        module: module.into(),
        parameters,
        tensors: 1,
        element_types: vec!["F32".into()],
        nominal_bytes: Some(0),
        routed_parameters: routed,
    }
}

fn record(cells: Vec<RecordCell>, experts: Option<(i64, i64)>) -> MorphometryRecord {
    MorphometryRecord {
        model: "m".into(),
        source: "TENSOR_INVENTORY".into(),
        schema_version: "alelyon.lattice.model-morphometry/0.1".into(),
        refusal: String::new(),
        gaps: Vec::new(),
        native_axis_order: ["block".into(), "module".into()],
        expert_count: experts.map(|(total, _)| total),
        expert_used_count: experts.map(|(_, used)| used),
        cells,
    }
}

#[test]
fn d2_an_operand_refused_for_int64_compares_as_unmeasured() {
    // The D2 refusal is the operand's own refusal, so the comparison inherits
    // it as INPUT_UNMEASURED rather than comparing a wrapped figure.
    let big = MetaValue::Int("9223372036854775808".into()); // 2^63
    let refused = payload(&[("general.parameter_count", big)], None);
    let fine = payload(
        &[
            ("general.architecture", MetaValue::Str("qwen3".into())),
            ("qwen3.block_count", int(1)),
        ],
        Some(vec![tensor("blk.0.attn_q.weight", "F32", &[8, 8])]),
    );
    let c = compare(Some(&refused), "big", Some(&fine), "fine").expect("comparison");
    assert_eq!(c.code, ComparisonCode::InputUnmeasured);
    assert_eq!(c.unmeasured_sides, vec!["left".to_owned()]);
    assert_eq!(
        c.explanation,
        "comparison requires two measured operands (left: INTEGER_OUT_OF_RANGE)"
    );
    assert!(c.left_gaps[0].contains("deviation D2"), "{:?}", c.left_gaps);
    assert!(!c.ok() && !c.complete() && c.cells.is_empty() && c.space_ref.is_empty());
}

#[test]
fn the_comparisons_integers_stay_exact_at_the_int64_extremes() {
    // Python's integers are unbounded; every figure the comparison derives from
    // validated int64 counts is within int64, so nothing here may refuse or wrap.
    let max = i64::MAX;
    let routed = max - max % 8; // divisible by the expert count
    let left = record(
        vec![
            cell(None, "output", 1, 0),
            cell(Some(0), "ffn_expert", max, routed),
        ],
        Some((8, 8)),
    );
    let right = record(
        vec![
            cell(None, "output", max, 0),
            cell(Some(0), "ffn_expert", 1, 0),
        ],
        Some((8, 8)),
    );
    let c = compare_records(&left, &right).expect("comparison");
    assert_eq!(c.code, ComparisonCode::Compared);
    let output = &c.cells[0];
    assert_eq!(output.key(), (None, "output"));
    assert_eq!(output.parameter_delta, Some(max - 1));
    assert_eq!(
        output.relative_parameter_difference,
        Some(Ratio(max - 1, 1))
    );
    let expert = &c.cells[1];
    assert_eq!(expert.parameter_delta, Some(1 - max));
    assert_eq!(expert.absolute_parameter_difference, Some(max - 1));
    // (1 - max) / max is already reduced (max is odd, gcd(max - 1, max) = 1).
    assert_eq!(
        expert.relative_parameter_difference,
        Some(Ratio(1 - max, max))
    );
    // All experts used: the whole cell is active, (routed * 8) // 8 without
    // forming routed * 8.
    assert_eq!(expert.left_active_parameters, Some(max));
    assert_eq!(expert.active_parameters_delta, Some(1 - max));
    assert!(c.complete());
}

#[test]
fn a_comparison_is_deterministic_and_orders_the_stack_external_row_first() {
    let left = record(
        vec![
            cell(Some(2), "attn_q", 4, 0),
            cell(None, "token_embd", 4, 0),
            cell(Some(0), "attn_q", 4, 0),
        ],
        None,
    );
    let right = record(vec![cell(Some(0), "attn_q", 6, 0)], None);
    let first = compare_records_json(&left, &right).expect("comparison");
    assert_eq!(
        first,
        compare_records_json(&left, &right).expect("comparison")
    );
    let c = compare_records(&left, &right).expect("comparison");
    let keys: Vec<_> = c.cells.iter().map(|cell| cell.key()).collect();
    assert_eq!(
        keys,
        vec![
            (None, "token_embd"),
            (Some(0), "attn_q"),
            (Some(2), "attn_q")
        ]
    );
    assert_eq!(c.cells[1].relative_parameter_difference, Some(Ratio(1, 2)));
    assert_eq!(c.cells[0].presence, CellPresence::LeftOnly);
    // Side-only cells carry no numeric delta, and the gap names them.
    assert_eq!(c.cells[0].parameter_delta, None);
    assert_eq!(c.cells[0].relative_parameter_difference, None);
    assert_eq!(
        c.gaps,
        vec!["2 canonical cell(s) were reported on the left only; absence is not measured zero"]
    );
    assert!(!c.complete());
}

// ── PR 5: the economics and the CPU package energy counter ──────────────────

#[test]
fn d14_a_comparison_needs_every_record_and_none_has_a_default() {
    use model_anatomy::{
        EnergyDeclaration, MonthlyVolume, ProviderPrice, ThroughputReading, compare_costs,
    };
    // The defaults are absent, never typical figures: Default/new hold no rate,
    // no wattage, no tariff, no price, no volume.
    let energy = EnergyDeclaration::default();
    assert_eq!((energy.draw_watts, energy.usd_per_kwh), (None, None));
    let volume = MonthlyVolume::default();
    assert_eq!((volume.input_tokens, volume.output_tokens), (None, None));
    let price = ProviderPrice::new("p", "m");
    assert_eq!(
        (price.usd_per_million_input, price.usd_per_million_output),
        (None, None)
    );
    let throughput = ThroughputReading::new("m", 0, None, None);
    let c = compare_costs(&throughput, &energy, Some(&price), &volume).expect("comparison");
    assert!(!c.established && c.local_usd.is_none() && c.hosted_usd.is_none());
    assert_eq!(c.refusals.len(), 4);
    assert!(
        c.describe
            .iter()
            .all(|line| line.starts_with("UNMEASURED: "))
    );
    assert!(!c.describe.iter().any(|line| line.contains('$')));
    assert_eq!(c.caveats.len(), 4, "the caveats travel with a refusal too");
}

#[test]
#[ignore = "reads this machine's CPU energy counter twice, 1 s apart (no device opened): run by hand"]
fn the_cpu_package_energy_counter_increases() {
    use model_anatomy::{CpuPackageEnergy, cpu_package_energy};
    let first = match cpu_package_energy().expect("the core answered") {
        CpuPackageEnergy::Read(energy) => energy,
        CpuPackageEnergy::Absent(reason) => panic!("the counter is absent here: {reason}"),
    };
    std::thread::sleep(std::time::Duration::from_secs(1));
    let second = cpu_package_energy()
        .expect("the core answered")
        .energy()
        .cloned()
        .expect("the counter read twice");
    assert_eq!(second.instance, "RAPL_Package0_PKG");
    assert!(
        second.picowatt_hours > first.picowatt_hours,
        "{} then {}",
        first.picowatt_hours,
        second.picowatt_hours
    );
    let picowatt_hours = second.picowatt_hours - first.picowatt_hours;
    let joules = picowatt_hours as f64 * 3.6e-9;
    let seconds = (second.filetime_100ns - first.filetime_100ns) as f64 / 1e7;
    println!(
        "{} {}\\{}: {} then {} pWh; {picowatt_hours} pWh = {joules:.3} J over {seconds:.4} s \
         (the counters' own timestamps) = {:.2} W",
        second.counter_set,
        second.instance,
        second.counter,
        first.picowatt_hours,
        second.picowatt_hours,
        joules / seconds
    );
    assert!(seconds > 0.0);
}

// ── PR 6: dequantisation and weight statistics (no Python counterpart) ─────

/// A valid synthetic GGUF file's bytes, taken from a weights golden.
fn golden_gguf(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
        .join("weights")
        .join(format!("{name}.json"));
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let hex = golden["input"]["gguf_hex"].as_str().unwrap();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

/// Writes `bytes` to a fresh file in the temporary folder and returns its path.
fn scratch_file(tag: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "model_anatomy_native_{}_{tag}.gguf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

fn refusal_of(tag: &str, bytes: &[u8]) -> model_anatomy::WeightStatistics {
    let path = scratch_file(tag, bytes);
    let result = model_anatomy::weight_statistics(&path, None, None);
    let _ = std::fs::remove_file(&path);
    result.unwrap()
}

#[test]
fn a_file_that_is_not_gguf_or_ends_in_its_header_is_refused_by_name() {
    let cases: Vec<(&str, Vec<u8>, &str)> = vec![
        ("empty", Vec::new(), "NOT_GGUF"),
        ("text", b"this is not a model file".to_vec(), "NOT_GGUF"),
        ("magic_only", b"GGUF".to_vec(), "HEADER_TRUNCATED"),
        ("v1", [b"GGUF".as_slice(), &1u32.to_le_bytes(), &[0u8; 16]].concat(), "UNSUPPORTED_VERSION"),
        ("half_header", golden_gguf("aligned_64")[..60].to_vec(), "HEADER_TRUNCATED"),
    ];
    for (tag, bytes, code) in cases {
        let result = refusal_of(tag, &bytes);
        let refusal = result.refusal.as_ref().unwrap_or_else(|| panic!("{tag}: not refused"));
        assert_eq!(refusal.code, code, "{tag}: {}", refusal.reason);
        assert!(result.tensors.is_empty() && result.cells.is_empty() && !result.complete, "{tag}");
    }
    // A path that does not exist.
    let missing = std::env::temp_dir().join("model_anatomy_native_no_such_file.gguf");
    let result = model_anatomy::weight_statistics(&missing, None, None).unwrap();
    assert_eq!(result.refusal.unwrap().code, "OPEN_FAILED");
    // A malformed alignment (zero is not a power of two).
    let mut bytes = golden_gguf("aligned_64");
    let key = b"general.alignment";
    let at = bytes
        .windows(key.len())
        .position(|w| w == key)
        .expect("the alignment key");
    let value = at + key.len() + 4; // past the value's type
    bytes[value..value + 4].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(refusal_of("zero_alignment", &bytes).refusal.unwrap().code, "MALFORMED_HEADER");
}

#[test]
fn a_type_without_a_dequantiser_and_partial_blocks_are_refused_by_name() {
    let types = model_anatomy::quant_types().unwrap();
    let q8_1 = types.iter().find(|t| t.name == "Q8_1").unwrap();
    assert!(!q8_1.supported);
    match model_anatomy::dequantize(q8_1.id, &[0u8; 40]) {
        Err(model_anatomy::Error::InvalidInput(reason)) => assert!(reason.contains("Q8_1"), "{reason}"),
        other => panic!("{other:?}"),
    }
    match model_anatomy::dequantize(12, &[0u8; 143]) {
        Err(model_anatomy::Error::InvalidInput(reason)) => assert!(reason.contains("Q4_K"), "{reason}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        model_anatomy::dequantize(999, &[]),
        Err(model_anatomy::Error::InvalidInput(_))
    ));
    assert_eq!(model_anatomy::dequantize(12, &[]).unwrap(), Vec::<f32>::new());
    for name in [
        "F32", "F16", "BF16", "Q8_0", "Q4_K", "Q5_K", "Q6_K", "IQ4_XS", "IQ3_S",
    ] {
        assert!(
            types.iter().any(|t| t.name == name && t.supported),
            "the reference model needs {name}"
        );
    }
}

#[test]
fn progress_reaches_the_total_on_the_calling_thread_and_cancel_refuses_the_run() {
    use std::sync::atomic::AtomicBool;
    let path = scratch_file("progress", &golden_gguf("mixed_types"));
    let caller = std::thread::current().id();
    let calls = std::cell::RefCell::new(Vec::new());
    let progress = |done: u64, total: u64| {
        assert_eq!(std::thread::current().id(), caller, "called on the calling thread");
        calls.borrow_mut().push((done, total));
    };
    let result = model_anatomy::weight_statistics(&path, Some(&progress), None).unwrap();
    let calls = calls.into_inner();
    let total = result.bytes_total.unwrap();
    assert_eq!(calls.first(), Some(&(0, total)));
    assert_eq!(calls.last(), Some(&(total, total)));
    assert!(calls.windows(2).all(|w| w[0].0 <= w[1].0), "{calls:?}");

    // A flag set before the run cancels it at the first callback.
    let cancel = AtomicBool::new(true);
    let result = model_anatomy::weight_statistics(&path, None, Some(&cancel)).unwrap();
    assert_eq!(result.refusal.unwrap().code, "CANCELLED");
    assert!(result.tensors.is_empty() && !result.complete);

    // A progress callback that panics cancels the run, and the panic comes back.
    let panicking = |_: u64, _: u64| panic!("the callback's own panic");
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        model_anatomy::weight_statistics(&path, Some(&panicking), None)
    }));
    let _ = std::fs::remove_file(&path);
    let payload = caught.expect_err("the panic is resumed");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"the callback's own panic"));
}

#[test]
fn the_histogram_edges_are_powers_of_two_from_two_to_the_minus_24() {
    use model_anatomy::{HISTOGRAM_BINS, histogram_bin_lower_edge};
    assert_eq!(HISTOGRAM_BINS, 32);
    assert_eq!(histogram_bin_lower_edge(0), Some(0.0));
    assert_eq!(histogram_bin_lower_edge(1), Some(2f64.powi(-24)));
    assert_eq!(histogram_bin_lower_edge(25), Some(1.0));
    assert_eq!(histogram_bin_lower_edge(31), Some(64.0));
    assert_eq!(histogram_bin_lower_edge(32), None);
    // One value per edge lands in its own bin, through a file the core reads.
    let mut values = vec![0.0f32, 1e-30, 2f32.powi(-24), 0.75, 1.0, 1.5, 63.99, 64.0, 1e30];
    values.push(-1.0);
    let path = scratch_file("edges", &f32_gguf(&values));
    let result = model_anatomy::weight_statistics(&path, None, None).unwrap();
    let _ = std::fs::remove_file(&path);
    let h = &result.tensors[0].stats.histogram;
    assert_eq!(h[0], 1, "1e-30 (0.0 is a zero, not binned): {h:?}");
    assert_eq!(h[1], 1, "2^-24");
    assert_eq!(h[24], 1, "0.75 in [2^-1, 1)");
    assert_eq!(h[25], 3, "1.0, 1.5 and -1.0 in [1, 2)");
    assert_eq!(h[30], 1, "63.99 in [32, 64)");
    assert_eq!(h[31], 2, "64 and 1e30");
    assert_eq!(result.tensors[0].stats.zeros, 1);
}

/// A minimal GGUF v3 file with one F32 tensor `blk.0.attn_norm.weight`.
fn f32_gguf(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GGUF");
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes()); // tensors
    out.extend_from_slice(&0u64.to_le_bytes()); // metadata
    let name = b"blk.0.attn_norm.weight";
    out.extend_from_slice(&(name.len() as u64).to_le_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(values.len() as u64).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // F32
    out.extend_from_slice(&0u64.to_le_bytes()); // offset
    while out.len() % 32 != 0 {
        out.push(0);
    }
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(windows)]
mod process_memory {
    //! The test process's own peak working set and peak private bytes, read
    //! through `K32GetProcessMemoryInfo` (kernel32, which std already links).
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }

    /// (peak working set, peak private bytes), or None if the call failed.
    pub fn peaks() -> Option<(usize, usize)> {
        let mut counters = Counters {
            cb: std::mem::size_of::<Counters>() as u32,
            ..Counters::default()
        };
        // SAFETY: the pseudo-handle of this process; `counters` is valid for
        // writes of `cb` bytes.
        let ok = unsafe {
            K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
        };
        (ok != 0).then_some((counters.peak_working_set_size, counters.peak_pagefile_usage))
    }
}

#[test]
#[ignore = "reads a whole model file named by MODEL_ANATOMY_WEIGHTS_FILE (read-only): run by hand, in release"]
fn weight_statistics_of_the_file_named_by_the_environment() {
    let path = std::env::var_os("MODEL_ANATOMY_WEIGHTS_FILE")
        .expect("set MODEL_ANATOMY_WEIGHTS_FILE to a GGUF file");
    let path = std::path::PathBuf::from(path);
    let calls = std::cell::Cell::new(0u64);
    let last = std::cell::Cell::new((0u64, 0u64));
    let progress = |done: u64, total: u64| {
        calls.set(calls.get() + 1);
        last.set((done, total));
    };
    let started = std::time::Instant::now();
    let json = model_anatomy::weight_statistics_json(&path, 0, Some(&progress), None).unwrap();
    let elapsed = started.elapsed().as_secs_f64();
    let result: model_anatomy::WeightStatistics = serde_json::from_str(&json).unwrap();
    println!(
        "progress: {} calls, last {:?}",
        calls.get(),
        last.get()
    );
    if let Some(refusal) = &result.refusal {
        panic!("refused: {} ({})", refusal.code, refusal.reason);
    }
    let bytes = result.bytes_total.unwrap();
    println!("file: {}", path.display());
    println!(
        "file bytes: {}; tensor data bytes read: {bytes}; tensors measured: {}; refused: {}; threads: {}",
        result.file_bytes,
        result.tensors.len(),
        result.refused.len(),
        result.threads.unwrap()
    );
    println!(
        "elapsed: {elapsed:.3} s; throughput: {:.3} GB/s (10^9 bytes of tensor data per second)",
        bytes as f64 / elapsed / 1e9
    );
    #[cfg(windows)]
    if let Some((working_set, private)) = process_memory::peaks() {
        println!(
            "peak working set: {:.1} MiB; peak private bytes: {:.1} MiB (this test process)",
            working_set as f64 / 1048576.0,
            private as f64 / 1048576.0
        );
    }
    for refused in &result.refused {
        println!("refused {} ({}): {} {}", refused.name, refused.type_name, refused.code, refused.reason);
    }
    println!("block module tensors count non_finite zeros mean std rms mean_abs min max");
    let rows: Vec<_> = result
        .cells
        .iter()
        .filter(|c| c.block.is_none() || c.block == Some(0) || c.block == Some(47))
        .collect();
    for cell in rows {
        let s = &cell.stats;
        println!(
            "{:?} {} {} {} {} {} {:.6e} {:.6e} {:.6e} {:.6e} {:?} {:?}",
            cell.block,
            cell.module,
            cell.tensors,
            s.count,
            s.non_finite,
            s.zeros,
            s.mean.unwrap_or(f64::NAN),
            s.std.unwrap_or(f64::NAN),
            s.rms.unwrap_or(f64::NAN),
            s.mean_abs.unwrap_or(f64::NAN),
            s.min,
            s.max
        );
    }
    // Optionally keep the whole document (for a cross-check outside the test).
    if let Some(dump) = std::env::var_os("MODEL_ANATOMY_WEIGHTS_JSON") {
        std::fs::write(&dump, &json).unwrap();
        println!("wrote the document to {}", std::path::Path::new(&dump).display());
    }
}
