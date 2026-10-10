//! Registration, the template hierarchy and the lineage layers (PR 2 of
//! ADR-0044), computed by the C++ core, and the transform families' facts.
//!
//! - [`register`] is the reachable subset of
//!   `registration.analyze_exact_compatibility`: identity and axis permutation
//!   (decided 2026-10-07). [`register_model`] is
//!   `morphometry.register(analyze(payload))`.
//! - [`template_hierarchy`] is `template_hierarchy.snapshot_template_hierarchy`
//!   (or, given no nodes, `model_morphometry_hierarchy()`).
//! - [`measure_layers`] is `hierarchy_morphometry.measure_layers`.
//! - [`transform_facts`] is a table GENERATED from `transforms.py` by
//!   `tools/model_anatomy_goldens.py` and embedded here; the C++ ports none of
//!   those families' logic beyond identity and permutation.
//!
//! The deviations (D6-D8) are named in the crate README.

use serde::Deserialize;

use super::{Error, Payload, VoxelField, ffi, take, with_payload};

/// `MAX_MATCHING_EDGE_VISITS`.
pub const MAX_MATCHING_EDGE_VISITS: i64 = 1 << 20;

/// `contracts.CoordinateAxis`, every field (enumerations as their values).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinateAxis {
    pub axis_id: String,
    pub semantic_id: String,
    pub kind: String,
    pub scalar_type: String,
    pub ordering: String,
    pub unit: Option<String>,
    pub reference_frame: Option<String>,
    pub calendar: Option<String>,
    pub timezone: Option<String>,
    pub orientation: Option<String>,
    pub origin: Option<String>,
    pub resolution: Option<String>,
    pub bounds: Option<(String, String)>,
    pub periodicity: Option<(String, String)>,
    pub labels_ref: Option<String>,
    pub labels: Option<Vec<String>>,
    pub missingness_policy: String,
    pub interpolation_policy: Vec<String>,
    pub transform_policy: Vec<String>,
    pub metadata: Vec<(String, String)>,
}

impl CoordinateAxis {
    /// An axis with the contract's defaults for every optional field.
    pub fn new(
        axis_id: &str,
        semantic_id: &str,
        kind: &str,
        scalar_type: &str,
        ordering: &str,
    ) -> Self {
        Self {
            axis_id: axis_id.into(),
            semantic_id: semantic_id.into(),
            kind: kind.into(),
            scalar_type: scalar_type.into(),
            ordering: ordering.into(),
            unit: None,
            reference_frame: None,
            calendar: None,
            timezone: None,
            orientation: None,
            origin: None,
            resolution: None,
            bounds: None,
            periodicity: None,
            labels_ref: None,
            labels: None,
            missingness_policy: "TYPED".into(),
            interpolation_policy: Vec::new(),
            transform_policy: Vec::new(),
            metadata: Vec::new(),
        }
    }
}

/// `contracts.CoordinateSpace`, every field. The core takes it as the contract
/// would construct it and does not re-validate it (D6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinateSpace {
    pub space_id: String,
    pub version: String,
    pub topology: String,
    pub axes: Vec<CoordinateAxis>,
    pub index_convention: String,
    pub unit_system: Option<String>,
    pub reference_frame: Option<String>,
    pub valid_domain_rule: String,
    pub region_atlas_refs: Vec<String>,
    pub metadata: Vec<(String, String)>,
}

/// A template's typed identity (not a content reference).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct TemplateRef {
    pub template_id: String,
    pub version: String,
}

impl TemplateRef {
    /// `TemplateRef.key`: `id@version`.
    pub fn key(&self) -> String {
        format!("{}@{}", self.template_id, self.version)
    }
}

/// `template_hierarchy.TemplateNode`, as given (the core normalises it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateNode {
    pub template_ref: TemplateRef,
    pub tier: String,
    pub label: String,
    pub description: String,
    pub parent_refs: Vec<TemplateRef>,
    pub coordinate_space: Option<CoordinateSpace>,
    pub transform_policy: Vec<String>,
    pub gaps: Vec<String>,
}

// ── lowering to the C structs ───────────────────────────────────────────────

/// Owns every array a lowered space or node points into. Each inner `Vec`'s
/// buffer stays where it is when the outer one grows, so a pointer taken into
/// one stays valid for the arena's life.
#[derive(Default)]
struct Arena {
    strs: Vec<Vec<ffi::MaStr>>,
    pairs: Vec<Vec<ffi::MaPair>>,
    axes: Vec<Vec<ffi::MaAxis>>,
    // Boxed on purpose: a node holds a pointer to its space, which must not
    // move when this Vec grows (`Vec<MaSpace>` would move it).
    #[allow(clippy::vec_box)]
    spaces: Vec<Box<ffi::MaSpace>>,
    refs: Vec<Vec<ffi::MaTemplateRef>>,
}

impl Arena {
    fn strs(&mut self, items: &[String]) -> (*const ffi::MaStr, usize) {
        let lowered: Vec<ffi::MaStr> = items.iter().map(|s| ffi::MaStr::of(s)).collect();
        let at = (lowered.as_ptr(), lowered.len());
        self.strs.push(lowered);
        at
    }

    fn pairs(&mut self, items: &[(String, String)]) -> (*const ffi::MaPair, usize) {
        let lowered: Vec<ffi::MaPair> = items
            .iter()
            .map(|(k, v)| ffi::MaPair {
                key: ffi::MaStr::of(k),
                value: ffi::MaStr::of(v),
            })
            .collect();
        let at = (lowered.as_ptr(), lowered.len());
        self.pairs.push(lowered);
        at
    }

    fn space(&mut self, space: &CoordinateSpace) -> *const ffi::MaSpace {
        let mut axes = Vec::with_capacity(space.axes.len());
        for axis in &space.axes {
            let (labels, labels_len) = self.strs(axis.labels.as_deref().unwrap_or(&[]));
            let (interpolation_policy, interpolation_policy_len) =
                self.strs(&axis.interpolation_policy);
            let (transform_policy, transform_policy_len) = self.strs(&axis.transform_policy);
            let (metadata, metadata_len) = self.pairs(&axis.metadata);
            let (bounds_lower, bounds_upper) = axis
                .bounds
                .as_ref()
                .map_or(("", ""), |(lo, hi)| (lo.as_str(), hi.as_str()));
            let (period, phase) = axis
                .periodicity
                .as_ref()
                .map_or(("", ""), |(p, q)| (p.as_str(), q.as_str()));
            axes.push(ffi::MaAxis {
                axis_id: ffi::MaStr::of(&axis.axis_id),
                semantic_id: ffi::MaStr::of(&axis.semantic_id),
                kind: ffi::MaStr::of(&axis.kind),
                scalar_type: ffi::MaStr::of(&axis.scalar_type),
                ordering: ffi::MaStr::of(&axis.ordering),
                unit: ffi::MaOptStr::of(axis.unit.as_deref()),
                reference_frame: ffi::MaOptStr::of(axis.reference_frame.as_deref()),
                calendar: ffi::MaOptStr::of(axis.calendar.as_deref()),
                timezone: ffi::MaOptStr::of(axis.timezone.as_deref()),
                orientation: ffi::MaOptStr::of(axis.orientation.as_deref()),
                origin: ffi::MaOptStr::of(axis.origin.as_deref()),
                resolution: ffi::MaOptStr::of(axis.resolution.as_deref()),
                has_bounds: u32::from(axis.bounds.is_some()),
                bounds_lower: ffi::MaStr::of(bounds_lower),
                bounds_upper: ffi::MaStr::of(bounds_upper),
                has_periodicity: u32::from(axis.periodicity.is_some()),
                period: ffi::MaStr::of(period),
                phase: ffi::MaStr::of(phase),
                labels_ref: ffi::MaOptStr::of(axis.labels_ref.as_deref()),
                has_labels: u32::from(axis.labels.is_some()),
                labels,
                labels_len,
                missingness_policy: ffi::MaStr::of(&axis.missingness_policy),
                interpolation_policy,
                interpolation_policy_len,
                transform_policy,
                transform_policy_len,
                metadata,
                metadata_len,
            });
        }
        let (region_atlas_refs, region_atlas_refs_len) = self.strs(&space.region_atlas_refs);
        let (metadata, metadata_len) = self.pairs(&space.metadata);
        let lowered = Box::new(ffi::MaSpace {
            space_id: ffi::MaStr::of(&space.space_id),
            version: ffi::MaStr::of(&space.version),
            topology: ffi::MaStr::of(&space.topology),
            axes: axes.as_ptr(),
            axes_len: axes.len(),
            index_convention: ffi::MaStr::of(&space.index_convention),
            unit_system: ffi::MaOptStr::of(space.unit_system.as_deref()),
            reference_frame: ffi::MaOptStr::of(space.reference_frame.as_deref()),
            valid_domain_rule: ffi::MaStr::of(&space.valid_domain_rule),
            region_atlas_refs,
            region_atlas_refs_len,
            metadata,
            metadata_len,
        });
        self.axes.push(axes);
        let at: *const ffi::MaSpace = &*lowered;
        self.spaces.push(lowered);
        at
    }

    fn template_ref(r: &TemplateRef) -> ffi::MaTemplateRef {
        ffi::MaTemplateRef {
            template_id: ffi::MaStr::of(&r.template_id),
            version: ffi::MaStr::of(&r.version),
        }
    }

    fn nodes(&mut self, nodes: &[TemplateNode]) -> Vec<ffi::MaTemplateNode> {
        nodes
            .iter()
            .map(|node| {
                let parents: Vec<ffi::MaTemplateRef> =
                    node.parent_refs.iter().map(Self::template_ref).collect();
                let (parent_refs, parent_refs_len) = (parents.as_ptr(), parents.len());
                self.refs.push(parents);
                let coordinate_space = match &node.coordinate_space {
                    Some(space) => self.space(space),
                    None => std::ptr::null(),
                };
                let (transform_policy, transform_policy_len) = self.strs(&node.transform_policy);
                let (gaps, gaps_len) = self.strs(&node.gaps);
                ffi::MaTemplateNode {
                    template_ref: Self::template_ref(&node.template_ref),
                    tier: ffi::MaStr::of(&node.tier),
                    label: ffi::MaStr::of(&node.label),
                    description: ffi::MaStr::of(&node.description),
                    parent_refs,
                    parent_refs_len,
                    coordinate_space,
                    transform_policy,
                    transform_policy_len,
                    gaps,
                    gaps_len,
                }
            })
            .collect()
    }
}

fn parse<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text)
        .map_err(|error| Error::Internal(format!("unexpected result: {error}")))
}

// ── registration ────────────────────────────────────────────────────────────

/// `analyze_exact_compatibility(source_space=source, target_space=target)` (the
/// reachable subset), as the core's JSON, under an edge-visit budget
/// ([`MAX_MATCHING_EDGE_VISITS`] is Python's).
pub fn register_json(
    source: &CoordinateSpace,
    target: &CoordinateSpace,
    edge_visit_budget: i64,
) -> Result<String, Error> {
    let mut arena = Arena::default();
    let source = arena.space(source);
    let target = arena.space(target);
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `source`/`target` and everything they point into are owned by
    // `arena` (and the borrowed spaces), alive until after the call; the core
    // only reads them. `out`/`len` are valid for writes.
    let code = unsafe { ffi::ma_register(source, target, edge_visit_budget, &mut out, &mut len) };
    take(code, out, len)
}

/// [`register_json`] with Python's budget, deserialised.
pub fn register(
    source: &CoordinateSpace,
    target: &CoordinateSpace,
) -> Result<CompatibilityReport, Error> {
    parse(&register_json(source, target, MAX_MATCHING_EDGE_VISITS)?)
}

/// `morphometry.register(analyze(payload, model=model))`, as JSON.
pub fn register_model_json(payload: Option<&Payload>, model: &str) -> Result<String, Error> {
    with_payload(payload, |raw| {
        let mut out = std::ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: `raw` and `model` outlive the call; null is allowed.
        let code =
            unsafe { ffi::ma_register_model(ffi::MaStr::of(model), raw, &mut out, &mut len) };
        take(code, out, len)
    })
}

/// [`register_model_json`], deserialised.
pub fn register_model(
    payload: Option<&Payload>,
    model: &str,
) -> Result<CompatibilityReport, Error> {
    parse(&register_model_json(payload, model)?)
}

/// `CompatibilityReport`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityReport {
    /// A `CompatibilityCode` value, or the native `NOT_PORTED` (D7).
    pub code: String,
    pub compatible: bool,
    pub explanation: String,
    pub transform: Option<TransformChain>,
    pub failing_constraint: Option<String>,
    pub evidence: Vec<String>,
    pub can_retry_with_metadata_or_policy: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransformChain {
    pub transforms: Vec<TransformStep>,
    pub loss_class: String,
    pub invertibility: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransformStep {
    pub transform_type: String,
    pub loss_class: String,
    pub invertibility: String,
    /// `source_order[j]` is the target axis supplying source axis j
    /// (AXIS_PERMUTATION only).
    pub source_order: Option<Vec<i64>>,
}

// ── the template hierarchy ──────────────────────────────────────────────────

/// `snapshot_template_hierarchy(nodes)`, or `model_morphometry_hierarchy()`
/// when `nodes` is `None`, as JSON.
pub fn template_hierarchy_json(nodes: Option<&[TemplateNode]>) -> Result<String, Error> {
    let mut arena = Arena::default();
    let lowered = nodes.map(|nodes| arena.nodes(nodes)).unwrap_or_default();
    let mut out = std::ptr::null_mut();
    let mut len = 0usize;
    // SAFETY: `lowered` and the arena it points into outlive the call.
    let code = unsafe {
        ffi::ma_template_hierarchy(
            u32::from(nodes.is_none()),
            lowered.as_ptr(),
            lowered.len(),
            &mut out,
            &mut len,
        )
    };
    take(code, out, len)
}

/// [`template_hierarchy_json`], deserialised.
pub fn template_hierarchy(nodes: Option<&[TemplateNode]>) -> Result<HierarchySnapshot, Error> {
    parse(&template_hierarchy_json(nodes)?)
}

/// `TemplateHierarchySnapshot`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HierarchySnapshot {
    pub schema_version: String,
    /// `EMPTY`, `INVALID` or `AVAILABLE`.
    pub state: String,
    pub nodes: Vec<SnapshotNode>,
    pub findings: Vec<HierarchyFinding>,
    pub gaps: Vec<String>,
    pub roots: Vec<TemplateRef>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SnapshotNode {
    #[serde(rename = "ref")]
    pub template_ref: TemplateRef,
    pub tier: String,
    pub label: String,
    pub description: String,
    pub parent_refs: Vec<TemplateRef>,
    pub coordinate_space: Option<SpaceSummary>,
    pub transform_policy: Vec<String>,
    pub gaps: Vec<String>,
}

/// A node's frame, as the hierarchy views show it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SpaceSummary {
    pub space_id: String,
    pub version: String,
    pub topology: String,
    pub axes: Vec<String>,
    pub index_convention: String,
    /// `coordinate_space_ref(space)`.
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HierarchyFinding {
    pub code: String,
    pub detail: String,
    pub node_ref: Option<TemplateRef>,
    pub parent_ref: Option<TemplateRef>,
}

// ── the lineage layers ──────────────────────────────────────────────────────

/// `measure_layers(morph, snapshot=...)`, as JSON. `morph` `None` is
/// `morph=None`; `Some((payload, model))` is `analyze(payload, model=model)`.
/// `nodes` `None` is the built-in hierarchy.
pub fn measure_layers_json(
    morph: Option<(Option<&Payload>, &str)>,
    nodes: Option<&[TemplateNode]>,
) -> Result<String, Error> {
    let mut arena = Arena::default();
    let lowered = nodes.map(|nodes| arena.nodes(nodes)).unwrap_or_default();
    let (payload, model) = morph.unwrap_or((None, ""));
    with_payload(payload, |raw| {
        let mut out = std::ptr::null_mut();
        let mut len = 0usize;
        // SAFETY: `raw`, `model`, `lowered` and the arena outlive the call.
        let code = unsafe {
            ffi::ma_measure_layers(
                u32::from(morph.is_some()),
                ffi::MaStr::of(model),
                raw,
                u32::from(nodes.is_none()),
                lowered.as_ptr(),
                lowered.len(),
                &mut out,
                &mut len,
            )
        };
        take(code, out, len)
    })
}

/// [`measure_layers_json`], deserialised.
pub fn measure_layers(
    morph: Option<(Option<&Payload>, &str)>,
    nodes: Option<&[TemplateNode]>,
) -> Result<HierarchyMorphometry, Error> {
    parse(&measure_layers_json(morph, nodes)?)
}

/// `HierarchyMorphometry`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HierarchyMorphometry {
    pub schema_version: String,
    pub model: String,
    pub hierarchy_state: String,
    pub model_measured: bool,
    /// Distinct volume objects across the drawn frames (0 or 1).
    pub volumes_built: i64,
    pub layers: Vec<HierarchyLayer>,
    pub findings: Vec<String>,
    pub gaps: Vec<String>,
    /// The one shared volume, when any frame drew it.
    pub field: Option<VoxelField>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HierarchyLayer {
    pub tier: String,
    pub order: i64,
    pub label: String,
    pub nodes: i64,
    pub abstract_nodes: i64,
    pub state: String,
    pub reason: String,
    pub drawn: bool,
    pub occupied: i64,
    pub frames: Vec<LayerFrame>,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LayerFrame {
    pub template_id: String,
    pub template_version: String,
    pub label: String,
    pub space_id: String,
    pub space_version: String,
    pub space_commitment: String,
    pub state: String,
    pub compatibility: String,
    pub explanation: String,
    pub transform_policy: Vec<String>,
    pub drawn: bool,
    pub occupied: i64,
}

// ── the transform facts table ───────────────────────────────────────────────

/// The generated table's text (`generated/transform_facts.json`).
pub const TRANSFORM_FACTS_JSON: &str = include_str!("../generated/transform_facts.json");

/// The transform families' declared facts, read from `transforms.py` by the
/// goldens tool (not computed here).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransformFacts {
    pub source: String,
    /// `LOSS_CLASS_RANK`, strongest first.
    pub loss_class_rank: Vec<(String, i64)>,
    /// Ordered by (loss rank, class name), as the Python registration view lists them.
    pub families: Vec<TransformFamily>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransformFamily {
    #[serde(rename = "class")]
    pub class_name: String,
    pub transform_type: String,
    pub loss_class: String,
    pub loss_rank: i64,
    pub invertibility: String,
    /// `inspect.getdoc` of the class, verbatim.
    pub doc: String,
}

/// [`TRANSFORM_FACTS_JSON`], deserialised.
pub fn transform_facts() -> Result<TransformFacts, Error> {
    parse(TRANSFORM_FACTS_JSON)
}
