//! The Morphometry tab's Reading: every measurement of a model (its header, its weights, one prompt's activations, a
//! comparison) turned into a few plain sentences a person can take in at a glance, each of which opens onto the figures
//! behind it, says how it is known, and says what it does not tell you (nuanced but
//! easily processed and digested).
//!
//! A finding is a sentence with four things attached:
//!
//! - its **basis**: COUNTED from the header, DECLARED by the file, DERIVED by arithmetic from those, MEASURED in the
//!   weights, OBSERVED in one run of one prompt, or UNMEASURED. A sentence never hides which;
//! - its **tone**: plain (a description), notable (worth a look) or caution (something is missing or wrong). Notable
//!   is never a judgement of quality: nothing here measures how good a model is;
//! - the **figures** behind it and a **caveat** (what it cannot say), shown when it is opened;
//! - its **focus**: the (block, module) cells it is about, which the volume can light up.
//!
//! The thresholds that make a finding notable are DECLARED below, each with its reason, and every finding that uses
//! one says so in its figures. Only the C++ engine's outputs are read: this file chooses words and ranks
//! them, and computes no new measurement beyond medians and ratios of the engine's figures.

use std::collections::HashMap;

use model_anatomy::{CellPresence, MorphometryComparison, WeightStatistics};

use super::morphometry::{BIT_BANDS, Measured, byte_size, grouped, si};
use super::probes::Activations;

// ------------------------------------------------------------------ declared thresholds

/// A part's weight scale (RMS) is "clearly different" when it is this many robust deviations (1.4826 × the median
/// absolute deviation) from the same part's median across blocks: the rule the engine's block dispersion uses.
pub const SCALE_DEVIATIONS: f64 = 3.0;
/// ... and at least this far from it as a ratio, so a part whose blocks are all near-identical is not flagged for a
/// difference nobody would notice.
pub const SCALE_RATIO: f64 = 1.5;
/// A part's largest weight is notable at this many times its RMS.
pub const TAIL_RATIO: f64 = 50.0;
/// Exact zeros are notable when they are this share of the weights or more.
pub const ZERO_SHARE: f64 = 0.01;
/// One layer's step in signal size (output RMS over the previous layer's) is notable at this ratio.
pub const SIGNAL_STEP: f64 = 3.0;
/// A layer "leans on one expert" when a single expert is chosen for this share of the prompt's tokens or more...
pub const LEANING_SHARE: f64 = 0.75;
/// ... over at least this many tokens (fewer is too few to call a lean).
pub const LEANING_TOKENS: i64 = 8;
/// The tensor list's count and the file's declared count differ notably beyond this share.
pub const COVERAGE_SLACK: f64 = 0.005;

// ------------------------------------------------------------------ the reading

/// How a finding is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Counted,
    Declared,
    Derived,
    Measured,
    Observed,
    Unmeasured,
}

impl Basis {
    pub const ALL: [Basis; 6] =
        [Basis::Counted, Basis::Declared, Basis::Derived, Basis::Measured, Basis::Observed, Basis::Unmeasured];

    pub fn word(self) -> &'static str {
        match self {
            Basis::Counted => "COUNTED",
            Basis::Declared => "DECLARED",
            Basis::Derived => "DERIVED",
            Basis::Measured => "MEASURED",
            Basis::Observed => "OBSERVED",
            Basis::Unmeasured => "UNMEASURED",
        }
    }

    /// What the word means, in a few words.
    pub fn meaning(self) -> &'static str {
        match self {
            Basis::Counted => "counted from the file's header",
            Basis::Declared => "what the file says about itself",
            Basis::Derived => "arithmetic on the above",
            Basis::Measured => "read from the weights themselves",
            Basis::Observed => "seen in one run of one prompt",
            Basis::Unmeasured => "could not be established",
        }
    }
}

/// Whether a finding is a description, worth a look, or a sign that something is missing or wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tone {
    Plain,
    Notable,
    Caution,
}

/// The cells a finding is about, as the volume lights them: (block, module), a block of None being outside the stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    Cells(Vec<(Option<i64>, String)>),
    Blocks(Vec<i64>),
}

impl Focus {
    pub fn covers(&self, block: Option<i64>, module: &str) -> bool {
        match self {
            Focus::Cells(cells) => cells.iter().any(|(b, m)| *b == block && m == module),
            Focus::Blocks(blocks) => block.is_some_and(|b| blocks.contains(&b)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    /// Stable within a reading, so an opened finding stays open when the reading is rebuilt.
    pub id: &'static str,
    /// What it is about, in one or two words ("Size", "Routing").
    pub topic: &'static str,
    pub basis: Basis,
    pub tone: Tone,
    pub headline: String,
    pub figures: Vec<String>,
    pub caveat: String,
    pub focus: Option<Focus>,
}

/// A reading of one model that could go further.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Further {
    Weights,
    Activations,
    Compare,
}

impl Further {
    pub fn offer(self) -> &'static str {
        match self {
            Further::Weights => "Read the weights themselves: their scale, extremes and zeros, part by part (reads the whole file)",
            Further::Activations => "Run a prompt through it and watch how the signal and the experts behave",
            Further::Compare => "Compare it with another installed model",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub model: String,
    /// Up to three headlines: what it is, how it works, and the first thing worth a look.
    pub glance: Vec<String>,
    pub findings: Vec<Finding>,
    pub further: Vec<Further>,
}

/// Everything known about one model.
pub struct Inputs<'a> {
    pub measured: &'a Measured,
    /// A module's human label ("Expert bank"), from the engine's constants.
    pub labels: &'a HashMap<String, String>,
    pub weights: Option<&'a WeightStatistics>,
    pub activations: Option<&'a Activations>,
    pub comparison: Option<&'a MorphometryComparison>,
    /// When the weights and the activations were read, in words ("2 hours ago"), and the probe's prompt.
    pub weights_when: Option<String>,
    pub activations_when: Option<String>,
    pub prompt: Option<&'a str>,
}

impl Inputs<'_> {
    /// A cell's words: "block 3 · Expert bank", or the module alone outside the stack.
    fn place(&self, block: Option<i64>, module: &str) -> String {
        let label = self.labels.get(module).cloned().unwrap_or_else(|| module.to_string());
        match block {
            Some(b) => format!("block {b} · {label}"),
            None => label,
        }
    }

    fn label(&self, module: &str) -> String {
        self.labels.get(module).cloned().unwrap_or_else(|| module.to_string())
    }
}

/// The reading of one model.
pub fn read(inputs: &Inputs) -> Reading {
    let mut findings = Vec::new();
    header(inputs, &mut findings);
    let measured = inputs.measured.analysis.morphometry.derived.ok;
    if measured {
        if let Some(w) = inputs.weights {
            let from = findings.len();
            weights(inputs, w, &mut findings);
            stamp(&mut findings[from..], inputs.weights_when.as_deref().map(|w| format!("Read from the file {w}")));
        }
        if let Some(a) = inputs.activations {
            let from = findings.len();
            activations(a, &mut findings);
            let mut line = inputs.activations_when.as_deref().map(|w| format!("Run {w}")).unwrap_or_default();
            if let Some(prompt) = inputs.prompt {
                let short: String = prompt.chars().take(90).collect();
                let more = if prompt.chars().count() > 90 { "…" } else { "" };
                line = format!("{line}{}the prompt \"{short}{more}\"", if line.is_empty() { "On " } else { ", on " });
            }
            stamp(&mut findings[from..], (!line.is_empty()).then_some(line));
        }
        if let Some(c) = inputs.comparison {
            comparison(inputs, c, &mut findings);
        }
    }
    gaps(inputs, &mut findings);
    let mut further = Vec::new();
    if measured {
        if inputs.weights.is_none() {
            further.push(Further::Weights);
        }
        if inputs.activations.is_none() {
            further.push(Further::Activations);
        }
        if inputs.comparison.is_none() {
            further.push(Further::Compare);
        }
    }
    Reading { model: inputs.measured.model.clone(), glance: glance(&findings), findings, further }
}

/// Put when (and on what) a probe's findings were taken first among their figures.
fn stamp(findings: &mut [Finding], line: Option<String>) {
    if let Some(line) = line {
        for f in findings {
            f.figures.insert(0, line.clone());
        }
    }
}

/// The two describing headlines (size, then how it works), then the first caution, else the first notable.
fn glance(findings: &[Finding]) -> Vec<String> {
    let mut out: Vec<String> =
        findings.iter().filter(|f| matches!(f.id, "size" | "work" | "refused")).map(|f| f.headline.clone()).collect();
    let worth = findings
        .iter()
        .filter(|f| !matches!(f.id, "size" | "work" | "refused"))
        .find(|f| f.tone == Tone::Caution)
        .or_else(|| findings.iter().filter(|f| !matches!(f.id, "size" | "work")).find(|f| f.tone == Tone::Notable));
    if let Some(f) = worth {
        out.push(f.headline.clone());
    }
    out.truncate(3);
    out
}

fn percent(share: f64) -> String {
    let p = share * 100.0;
    if p > 0.0 && p < 0.1 {
        "under 0.1%".to_string()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let n = values.len();
    Some(if n % 2 == 1 { values[n / 2] } else { (values[n / 2 - 1] + values[n / 2]) / 2.0 })
}

/// A list of block numbers as runs: "0-3, 7, 9-10".
fn runs(blocks: &[i64]) -> String {
    let mut sorted = blocks.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j + 1 < sorted.len() && sorted[j + 1] == sorted[j] + 1 {
            j += 1;
        }
        parts.push(if i == j { sorted[i].to_string() } else { format!("{}-{}", sorted[i], sorted[j]) });
        i = j + 1;
    }
    parts.join(", ")
}

/// What a family of parts does, in a phrase.
fn family_role(family: &str) -> &'static str {
    match family {
        "embedding" => "turns each token into a vector",
        "normalisation" => "rescales the signal between steps",
        "attention" => "lets each token draw on the others",
        "feed-forward" => "transforms each token on its own",
        "output" => "turns the last vector into next-token scores",
        _ => "unclassified",
    }
}

// ------------------------------------------------------------------ from the header

fn header(inputs: &Inputs, out: &mut Vec<Finding>) {
    let morph = &inputs.measured.analysis.morphometry;
    let d = &morph.derived;
    let name = if morph.model.is_empty() { inputs.measured.model.as_str() } else { morph.model.as_str() };
    if !d.ok {
        out.push(Finding {
            id: "refused",
            topic: "Not measured",
            basis: Basis::Unmeasured,
            tone: Tone::Caution,
            headline: format!(
                "{name} could not be measured from its header: {}.",
                if morph.refusal.is_empty() { "no cells were produced" } else { morph.refusal.as_str() }
            ),
            figures: Vec::new(),
            caveat: "Nothing below describes this model; the named gaps say what was missing.".to_string(),
            focus: None,
        });
        return;
    }
    let counted = d.counted_parameters;
    let counted_basis = if morph.source == "TENSOR_INVENTORY" { Basis::Counted } else { Basis::Derived };

    // Size.
    let bits = d.nominal_bytes.filter(|_| counted > 0).map(|b| b as f64 * 8.0 / counted as f64);
    let headline = match bits {
        Some(b) => format!(
            "{name} has {} parameters, stored in {}: about {:.1} bits each.",
            si(Some(counted)),
            byte_size(d.nominal_bytes),
            b
        ),
        None => format!("{name} has {} parameters; how much space they take is UNMEASURED.", si(Some(counted))),
    };
    let mut figures = vec![
        if counted_basis == Basis::Counted {
            format!("{} parameters counted from the header's list of {} tensors", grouped(counted), grouped(morph.tensors.len() as i64))
        } else {
            format!("{} parameters computed from the declared architecture (the header lists no tensors)", grouped(counted))
        },
        format!("The file declares {} parameters", morph.declared_parameters.map_or("no count of its".to_string(), grouped)),
        format!("{} blocks (layers) in the stack", d.blocks.len()),
    ];
    if !morph.quantization.is_empty() {
        figures.push(format!("Storage format label: {}", morph.quantization));
    }
    if let Some(n) = morph.context_length.filter(|n| *n > 0) {
        figures.push(format!("Declared context length: {} tokens", grouped(n)));
    }
    out.push(Finding {
        id: "size",
        topic: "Size",
        basis: counted_basis,
        tone: Tone::Plain,
        headline,
        figures,
        caveat: "Size says nothing about how capable the model is. Storage is the nominal size of the tensors' formats; \
                 the file also holds its header and padding."
            .to_string(),
        focus: None,
    });

    // The two counts disagree.
    if let Some(coverage) = d.coverage.filter(|c| (c - 1.0).abs() > COVERAGE_SLACK) {
        out.push(Finding {
            id: "coverage",
            topic: "Counts disagree",
            basis: Basis::Counted,
            tone: Tone::Caution,
            headline: format!("The tensors listed add up to {} of the parameter count the file declares.", percent(coverage)),
            figures: vec![
                format!("Counted: {}", grouped(counted)),
                format!("Declared: {}", morph.declared_parameters.map_or("-".to_string(), grouped)),
                format!("Flagged beyond {} either way (declared threshold)", percent(COVERAGE_SLACK)),
            ],
            caveat: "One of the two counts is wrong or counts differently (a shared embedding, for one); this view does \
                     not decide which."
                .to_string(),
            focus: None,
        });
    }

    // How it works: dense, or a mixture of experts.
    if d.is_mixture_of_experts {
        let experts: Vec<(Option<i64>, String)> =
            morph.cells.iter().filter(|c| c.routed_parameters > 0).map(|c| (c.block, c.module.clone())).collect();
        match d.active_parameters {
            Some(active) => out.push(Finding {
                id: "work",
                topic: "How it works",
                basis: Basis::Derived,
                tone: Tone::Notable,
                headline: format!(
                    "A mixture of experts: each token passes through about {} of its {} parameters ({}).",
                    si(Some(active)),
                    si(Some(counted)),
                    percent(d.active_fraction.unwrap_or(0.0))
                ),
                figures: vec![
                    format!(
                        "{} experts in each block; {} chosen for each token (declared)",
                        morph.expert_count.map_or("-".to_string(), grouped),
                        morph.expert_used_count.map_or("-".to_string(), grouped)
                    ),
                    format!("The experts hold {} parameters", si(d.routed_expert_parameters)),
                    format!("Every token uses the other {}", si(d.always_active_parameters)),
                ],
                caveat: "This is the declared routing. Which experts a token actually reaches is decided as it runs (the \
                         activation probe watches it for one prompt). Fewer parameters per token means less work per \
                         token, not a smaller download: every expert is still stored."
                    .to_string(),
                focus: (!experts.is_empty()).then_some(Focus::Cells(experts)),
            }),
            None => out.push(Finding {
                id: "work",
                topic: "How it works",
                basis: Basis::Unmeasured,
                tone: Tone::Caution,
                headline: "A mixture of experts, but how much of it each token passes through is UNMEASURED.".to_string(),
                figures: vec![d.active_path_gap.clone().unwrap_or_else(|| {
                    "The runtime did not establish a usable expert partition.".to_string()
                })],
                caveat: "Without the expert count and the experts used per token, the share of the work cannot be \
                         separated from the share of storage."
                    .to_string(),
                focus: (!experts.is_empty()).then_some(Focus::Cells(experts)),
            }),
        }
    } else {
        out.push(Finding {
            id: "work",
            topic: "How it works",
            basis: Basis::Derived,
            tone: Tone::Plain,
            headline: format!("Dense: every token passes through all {} parameters.", si(Some(counted))),
            figures: vec!["No tensor in the header is a routed expert bank.".to_string()],
            caveat: "Read from the header's tensor names and shapes.".to_string(),
            focus: None,
        });
    }

    // Where the parameters sit.
    let mut families = d.by_family.clone();
    families.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some((top, top_count)) = families.first().filter(|_| counted > 0) {
        let share = |n: &i64| *n as f64 / counted as f64;
        let mut headline = format!("Most of it is {top} ({}), the part that {}", percent(share(top_count)), family_role(top));
        if let Some((second, second_count)) = families.get(1) {
            headline.push_str(&format!("; then {second} ({})", percent(share(second_count))));
        }
        headline.push('.');
        let figures = families
            .iter()
            .map(|(f, n)| format!("{f}: {} parameters, {} · {}", si(Some(*n)), percent(share(n)), family_role(f)))
            .collect();
        let cells: Vec<(Option<i64>, String)> =
            morph.cells.iter().filter(|c| &c.family == top).map(|c| (c.block, c.module.clone())).collect();
        out.push(Finding {
            id: "layout",
            topic: "Where it sits",
            basis: counted_basis,
            tone: Tone::Plain,
            headline,
            figures,
            caveat: if d.is_mixture_of_experts {
                "A share of what is STORED, not of the work: a token reaches only some of the experts.".to_string()
            } else {
                "A share of what is stored; on a dense model every token uses all of it.".to_string()
            },
            focus: (!cells.is_empty()).then_some(Focus::Cells(cells)),
        });
    }

    // Depth: are the blocks built alike?
    let n = d.blocks.len();
    match d.block_dispersion.relative {
        None if n > 0 => out.push(Finding {
            id: "depth",
            topic: "Depth",
            basis: Basis::Unmeasured,
            tone: Tone::Plain,
            headline: format!("Whether its {n} blocks are built alike is UNMEASURED: it takes at least three."),
            figures: Vec::new(),
            caveat: String::new(),
            focus: None,
        }),
        None => {}
        Some(relative) => {
            let outliers = &d.block_dispersion.outliers;
            let mut sizes: Vec<f64> = d.blocks.iter().map(|b| b.parameters as f64).collect();
            let typical = median(&mut sizes).unwrap_or(0.0);
            let mut figures = vec![
                format!("A typical block holds {} parameters", si(Some(typical as i64))),
                format!("Blocks differ from the typical one by {} (median absolute deviation)", percent(relative)),
            ];
            for b in d.blocks.iter().filter(|b| outliers.contains(&b.block)).take(8) {
                figures.push(format!("Block {}: {} parameters", b.block, si(Some(b.parameters))));
            }
            let (headline, tone) = if outliers.is_empty() {
                (format!("All {n} blocks are built alike."), Tone::Plain)
            } else {
                (
                    format!(
                        "{} of its {n} blocks are built differently from the rest (block{} {}).",
                        outliers.len(),
                        if outliers.len() == 1 { "" } else { "s" },
                        runs(outliers)
                    ),
                    Tone::Notable,
                )
            };
            figures.push("A block is flagged beyond three deviations from the median, the engine's rule".to_string());
            out.push(Finding {
                id: "depth",
                topic: "Depth",
                basis: counted_basis,
                tone,
                headline,
                figures,
                caveat: "Structure only: blocks built alike can still have learned very different things.".to_string(),
                focus: (!outliers.is_empty()).then(|| Focus::Blocks(outliers.clone())),
            });
        }
    }

    // Precision.
    precision(inputs, out);
}

fn precision(inputs: &Inputs, out: &mut Vec<Finding>) {
    let morph = &inputs.measured.analysis.morphometry;
    let band_of = |bits: f64| BIT_BANDS.iter().position(|(ceiling, _, _)| bits < *ceiling).unwrap_or(BIT_BANDS.len() - 1);
    // Parameters per band (the last slot is UNMEASURED), and the modules seen in each.
    let mut shares = vec![0i64; BIT_BANDS.len() + 1];
    let mut modules: Vec<Vec<String>> = vec![Vec::new(); BIT_BANDS.len() + 1];
    for c in &morph.cells {
        let slot = c.mean_bits_per_weight.map_or(BIT_BANDS.len(), |r| band_of(r.0 as f64 / r.1 as f64));
        shares[slot] += c.parameters;
        let label = inputs.label(&c.module);
        if !modules[slot].contains(&label) {
            modules[slot].push(label);
        }
    }
    let total: i64 = shares.iter().sum();
    if total <= 0 {
        return;
    }
    let words = |slot: usize| if slot == BIT_BANDS.len() { "UNMEASURED precision" } else { BIT_BANDS[slot].2 };
    let mut order: Vec<usize> = (0..shares.len()).filter(|s| shares[*s] > 0).collect();
    order.sort_by(|a, b| shares[*b].cmp(&shares[*a]));
    let main = order[0];
    let share = |slot: usize| shares[slot] as f64 / total as f64;
    let figures = order
        .iter()
        .map(|s| {
            let mut m = modules[*s].clone();
            let more = m.len().saturating_sub(4);
            m.truncate(4);
            format!("{}: {} of the parameters · {}{}", words(*s), percent(share(*s)), m.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() })
        })
        .collect();
    let (headline, tone) = if order.len() == 1 {
        (format!("Stored at one precision throughout: {}.", words(main)), Tone::Plain)
    } else {
        let rest: Vec<String> = order[1..].iter().take(2).map(|s| format!("{} at {}", percent(share(*s)), words(*s))).collect();
        (
            format!("Mixed precision: {} of the weights at {}, {}.", percent(share(main)), words(main), rest.join(", ")),
            if shares[BIT_BANDS.len()] > 0 { Tone::Caution } else { Tone::Plain },
        )
    };
    // The exceptions to the main band are what is worth seeing.
    let exceptions: Vec<(Option<i64>, String)> = morph
        .cells
        .iter()
        .filter(|c| c.mean_bits_per_weight.map_or(BIT_BANDS.len(), |r| band_of(r.0 as f64 / r.1 as f64)) != main)
        .map(|c| (c.block, c.module.clone()))
        .collect();
    out.push(Finding {
        id: "precision",
        topic: "Precision",
        basis: Basis::Counted,
        tone,
        headline,
        figures,
        caveat: "Nominal bits per weight, from each tensor's storage format. How much accuracy that precision costs is not \
                 measured here."
            .to_string(),
        focus: (!exceptions.is_empty()).then_some(Focus::Cells(exceptions)),
    });
}

fn gaps(inputs: &Inputs, out: &mut Vec<Finding>) {
    let morph = &inputs.measured.analysis.morphometry;
    if morph.gaps.is_empty() {
        return;
    }
    out.push(Finding {
        id: "gaps",
        topic: "Not established",
        basis: Basis::Unmeasured,
        tone: Tone::Caution,
        headline: format!(
            "{} thing{} could not be established from the header.",
            morph.gaps.len(),
            if morph.gaps.len() == 1 { "" } else { "s" }
        ),
        figures: morph.gaps.clone(),
        caveat: "A gap is not a zero: the figures it would have given are left out, never guessed.".to_string(),
        focus: None,
    });
}

// ------------------------------------------------------------------ from the weights

fn weights(inputs: &Inputs, w: &WeightStatistics, out: &mut Vec<Finding>) {
    if let Some(r) = &w.refusal {
        out.push(Finding {
            id: "weights-refused",
            topic: "Weights",
            basis: Basis::Unmeasured,
            tone: Tone::Caution,
            headline: format!("The weights could not be read: {}.", r.reason.trim_end_matches('.')),
            figures: vec![format!("Refusal: {}", r.code)],
            caveat: String::new(),
            focus: None,
        });
        return;
    }
    if !w.complete || !w.refused.is_empty() {
        let mut figures: Vec<String> =
            w.refused.iter().take(8).map(|r| format!("{} ({}): {}", r.name, r.type_name, r.reason)).collect();
        if w.refused.len() > 8 {
            figures.push(format!("and {} more", w.refused.len() - 8));
        }
        out.push(Finding {
            id: "weights-partial",
            topic: "Weights",
            basis: Basis::Unmeasured,
            tone: Tone::Caution,
            headline: if w.complete {
                format!("{} tensor{} could not be decoded; the weight findings leave them out.", w.refused.len(), if w.refused.len() == 1 { "" } else { "s" })
            } else {
                "The weight reading stopped part way; the weight findings cover only what was read.".to_string()
            },
            figures,
            caveat: String::new(),
            focus: None,
        });
    }
    let cells: Vec<_> = w.cells.iter().filter(|c| c.stats.count > 0).collect();
    if cells.is_empty() {
        return;
    }

    // Health: anything not a number.
    let count: u64 = cells.iter().map(|c| c.stats.count).sum();
    let broken: Vec<_> = cells.iter().filter(|c| c.stats.non_finite > 0).collect();
    if broken.is_empty() {
        out.push(Finding {
            id: "finite",
            topic: "Health",
            basis: Basis::Measured,
            tone: Tone::Plain,
            headline: format!("All {} weights read are ordinary finite numbers.", si(Some(count as i64))),
            figures: vec![format!("{} cells, every tensor dequantised as gguf-py does", cells.len())],
            caveat: "Finite is the least a weight can be; it says nothing about whether the values are good ones.".to_string(),
            focus: None,
        });
    } else {
        let bad: u64 = broken.iter().map(|c| c.stats.non_finite).sum();
        out.push(Finding {
            id: "finite",
            topic: "Health",
            basis: Basis::Measured,
            tone: Tone::Caution,
            headline: format!("{} weights are not numbers (NaN or infinite), in {} part{}.", grouped(bad as i64), broken.len(), if broken.len() == 1 { "" } else { "s" }),
            figures: broken.iter().take(8).map(|c| format!("{}: {}", inputs.place(c.block, &c.module), grouped(c.stats.non_finite as i64))).collect(),
            caveat: "A model with such weights usually produces broken output wherever they are used; the file may be damaged.".to_string(),
            focus: Some(Focus::Cells(broken.iter().map(|c| (c.block, c.module.clone())).collect())),
        });
    }

    // Scale: the same part, block to block.
    let mut by_module: HashMap<&str, Vec<(i64, f64)>> = HashMap::new();
    for c in &cells {
        if let (Some(b), Some(rms)) = (c.block, c.stats.rms.filter(|r| r.is_finite() && *r > 0.0)) {
            by_module.entry(c.module.as_str()).or_default().push((b, rms));
        }
    }
    let mut flagged: Vec<(i64, &str, f64)> = Vec::new();
    for (module, values) in &by_module {
        if values.len() < 3 {
            continue;
        }
        let mut v: Vec<f64> = values.iter().map(|x| x.1).collect();
        let Some(mid) = median(&mut v) else { continue };
        let mut deviations: Vec<f64> = values.iter().map(|x| (x.1 - mid).abs()).collect();
        let mad = median(&mut deviations).unwrap_or(0.0) * 1.4826;
        for (block, rms) in values {
            let ratio = rms / mid;
            let far = if mad > 0.0 { (rms - mid).abs() > SCALE_DEVIATIONS * mad } else { (rms - mid).abs() > 0.0 };
            if far && (ratio >= SCALE_RATIO || ratio <= 1.0 / SCALE_RATIO) {
                flagged.push((*block, module, ratio));
            }
        }
    }
    flagged.sort_by(|a, b| b.2.ln().abs().total_cmp(&a.2.ln().abs()).then(a.0.cmp(&b.0)));
    let times = |r: f64| if r >= 1.0 { format!("{r:.1}× the usual") } else { format!("{:.1}× smaller than usual", 1.0 / r) };
    // The flags grouped by part, the part with the largest departure first: a part that drifts with depth reads as one
    // pattern ("Attention norm in blocks 40-47"), not as eight separate findings.
    let mut groups: Vec<(&str, Vec<i64>, f64)> = Vec::new();
    for (b, m, r) in &flagged {
        match groups.iter_mut().find(|g| g.0 == *m) {
            Some(g) => g.1.push(*b),
            None => groups.push((m, vec![*b], *r)),
        }
    }
    let group_words = |g: &(&str, Vec<i64>, f64)| {
        let most = if g.2 >= 1.0 { "up to" } else { "down to" };
        format!("{} in block{} {} ({most} {})", inputs.label(g.0), if g.1.len() == 1 { "" } else { "s" }, runs(&g.1), times(g.2))
    };
    let mut figures = vec![format!(
        "Each part's weight scale (RMS) is set beside the same part in every other block; flagged at {SCALE_DEVIATIONS} \
         robust deviations and {SCALE_RATIO}× from the median (declared thresholds)"
    )];
    figures.extend(flagged.iter().take(8).map(|(b, m, r)| format!("{}: {}", inputs.place(Some(*b), m), times(*r))));
    if flagged.len() > 8 {
        figures.push(format!("and {} more", flagged.len() - 8));
    }
    out.push(Finding {
        id: "scale",
        topic: "Weight scale",
        basis: Basis::Measured,
        tone: if flagged.is_empty() { Tone::Plain } else { Tone::Notable },
        headline: match groups.len() {
            0 => "Each kind of part keeps a similar weight scale from block to block.".to_string(),
            1 => format!("One kind of part changes scale with depth: {}.", group_words(&groups[0])),
            n => format!(
                "{n} kinds of part change scale with depth: {}{}.",
                groups.iter().take(2).map(group_words).collect::<Vec<_>>().join("; "),
                if n > 2 { format!("; and {} more", n - 2) } else { String::new() }
            ),
        },
        figures,
        caveat: format!(
            "A fact about the numbers, not a defect: models often scale their first or last blocks differently on \
             purpose. Storage formats also round small weights, which moves the scale a little.{}",
            if groups.iter().any(|g| g.0.ends_with("norm")) {
                " A norm is a gain on each channel, not a matrix: its size sets how loudly the next step hears the signal."
            } else {
                ""
            }
        ),
        focus: (!flagged.is_empty()).then(|| Focus::Cells(flagged.iter().map(|(b, m, _)| (Some(*b), m.to_string())).collect())),
    });

    // Extremes: the largest single weights against their part's typical size.
    let mut tails: Vec<(Option<i64>, &str, f64)> = cells
        .iter()
        .filter(|c| c.stats.count >= 1024)
        .filter_map(|c| {
            let s = &c.stats;
            let largest = s.min?.abs().max(s.max?.abs());
            let rms = s.rms.filter(|r| *r > 0.0)?;
            Some((c.block, c.module.as_str(), largest / rms))
        })
        .collect();
    tails.sort_by(|a, b| b.2.total_cmp(&a.2));
    if let Some((b, m, ratio)) = tails.first() {
        let heavy: Vec<_> = tails.iter().filter(|t| t.2 >= TAIL_RATIO).collect();
        let mut figures = vec![format!(
            "Largest |weight| over the part's RMS; notable at {TAIL_RATIO:.0}× (declared threshold). {} part{} reach it.",
            heavy.len(),
            if heavy.len() == 1 { "" } else { "s" }
        )];
        figures.extend(tails.iter().take(6).map(|(b, m, r)| format!("{}: {r:.0}×", inputs.place(*b, m))));
        out.push(Finding {
            id: "extremes",
            topic: "Extremes",
            basis: Basis::Measured,
            tone: if heavy.is_empty() { Tone::Plain } else { Tone::Notable },
            headline: format!("Its largest single weights sit in {}, at {ratio:.0}× that part's typical size.", inputs.place(*b, m)),
            figures,
            caveat: "A few very large weights are common in trained models, and they are the hardest to keep when a model \
                     is stored in fewer bits. Whether these ones matter for output is not measured here."
                .to_string(),
            focus: Some(Focus::Cells(
                if heavy.is_empty() { tails.iter().take(3).collect::<Vec<_>>() } else { heavy }
                    .iter()
                    .map(|(b, m, _)| (*b, m.to_string()))
                    .collect(),
            )),
        });
    }

    // Exact zeros.
    let zeros: u64 = cells.iter().map(|c| c.stats.zeros).sum();
    let share = zeros as f64 / count as f64;
    let most = cells.iter().max_by_key(|c| c.stats.zeros).filter(|c| c.stats.zeros > 0);
    out.push(Finding {
        id: "zeros",
        topic: "Zeros",
        basis: Basis::Measured,
        tone: if share >= ZERO_SHARE { Tone::Notable } else { Tone::Plain },
        headline: if zeros == 0 {
            "No weight is exactly zero.".to_string()
        } else if share >= ZERO_SHARE {
            format!(
                "{} of its weights are exactly zero, most of them in {}.",
                percent(share),
                most.map_or(String::new(), |c| inputs.place(c.block, &c.module))
            )
        } else {
            format!("Almost no weight is exactly zero ({}).", percent(share))
        },
        figures: vec![
            format!("{} exact zeros of {} weights", grouped(zeros as i64), grouped(count as i64)),
            format!("Notable at {} or more (declared threshold)", percent(ZERO_SHARE)),
        ],
        caveat: "In a low-bit format a zero is often a small weight rounded down, not a weight that was removed.".to_string(),
        focus: most.filter(|_| share >= ZERO_SHARE).map(|c| Focus::Cells(vec![(c.block, c.module.clone())])),
    });
}

// ------------------------------------------------------------------ from one prompt

fn activations(a: &Activations, out: &mut Vec<Finding>) {
    let prompt = format!("one prompt of {} token{}", a.tokens, if a.tokens == 1 { "" } else { "s" });
    // The signal through the layers; the last layer is computed only for the output token, so it is left out.
    let sizes: Vec<(i64, f64)> =
        (0..a.layers.saturating_sub(1)).filter_map(|l| a.at("l_out", l).and_then(|t| t.rms).map(|r| (l, r))).collect();
    if sizes.len() >= 2 && let (Some(first), Some(last)) = (sizes.first(), sizes.last()) {
        let growth = last.1 / first.1;
        let step = sizes.windows(2).filter(|w| w[0].1 > 0.0).map(|w| (w[1].0, w[1].1 / w[0].1)).max_by(|x, y| x.1.total_cmp(&y.1));
        let mut headline = if growth >= 1.0 {
            format!("Through its layers the signal grows {}×", if growth >= 10.0 { format!("{growth:.0}") } else { format!("{growth:.1}") })
        } else {
            format!("Through its layers the signal shrinks to {:.2} of its size", growth)
        };
        if let Some((layer, ratio)) = step.filter(|s| s.1 >= SIGNAL_STEP) {
            headline.push_str(&format!(", with one sharp step at layer {layer} (×{ratio:.1})"));
        }
        headline.push('.');
        let mut figures = vec![
            format!("Output RMS {:.3} after layer {} → {:.3} after layer {}", first.1, first.0, last.1, last.0),
            format!("A step is notable at ×{SIGNAL_STEP:.0} over the layer before (declared threshold)"),
        ];
        let mut steps: Vec<(i64, f64)> = sizes.windows(2).filter(|w| w[0].1 > 0.0).map(|w| (w[1].0, w[1].1 / w[0].1)).collect();
        steps.sort_by(|x, y| y.1.total_cmp(&x.1));
        figures.extend(steps.iter().take(4).map(|(l, r)| format!("Layer {l}: ×{r:.2}")));
        let sharp = step.filter(|s| s.1 >= SIGNAL_STEP);
        out.push(Finding {
            id: "signal",
            topic: "Signal",
            basis: Basis::Observed,
            tone: if sharp.is_some() { Tone::Notable } else { Tone::Plain },
            headline,
            figures,
            caveat: format!(
                "From {prompt}: another prompt gives other numbers. Growth through the layers is normal in this kind of \
                 model; the last layer is left out because it is computed only for the token the model outputs."
            ),
            focus: sharp.map(|(l, _)| Focus::Blocks(vec![l])),
        });
    }
    let broken: Vec<String> = a.tensors.iter().filter(|t| t.nonfinite > 0).map(|t| format!("{}: {}", t.name, t.nonfinite)).collect();
    if !broken.is_empty() {
        out.push(Finding {
            id: "signal-broken",
            topic: "Signal",
            basis: Basis::Observed,
            tone: Tone::Caution,
            headline: format!("{} recorded activation{} held NaN or infinite values.", broken.len(), if broken.len() == 1 { "" } else { "s" }),
            figures: broken.into_iter().take(8).collect(),
            caveat: format!("From {prompt}."),
            focus: None,
        });
    }

    // Routing.
    let Some(experts) = a.expert_count.filter(|e| *e > 0) else { return };
    if a.experts.is_empty() {
        return;
    }
    let mut distinct = 0i64;
    let mut leaning: Vec<(i64, String, f64)> = Vec::new();
    for r in &a.experts {
        distinct += r.counts.len() as i64;
        if a.tokens >= LEANING_TOKENS
            && let Some((id, n)) = r.counts.iter().max_by(|x, y| x.1.cmp(y.1))
        {
            let share = *n as f64 / a.tokens as f64;
            if share >= LEANING_SHARE {
                leaning.push((r.layer, id.clone(), share));
            }
        }
    }
    let layers = a.experts.len() as i64;
    let average = distinct as f64 / layers as f64;
    leaning.sort_by(|x, y| y.2.total_cmp(&x.2).then(x.0.cmp(&y.0)));
    let mut headline = format!("On this prompt each layer used {average:.0} of its {experts} experts on average");
    match leaning.first() {
        None => headline.push_str("; no layer leaned on one expert for most tokens."),
        Some((layer, id, share)) => headline.push_str(&format!(
            "; {} layer{} leaned on a single expert for most tokens (layer {layer}: expert {id} for {} of them).",
            leaning.len(),
            if leaning.len() == 1 { "" } else { "s" },
            percent(*share)
        )),
    }
    let used = a.experts.first().map_or(0, |r| r.used_per_token);
    let reachable = experts.min(a.tokens.saturating_mul(used));
    let mut figures = vec![
        format!("{used} chosen per token; {} tokens over {layers} layers", a.tokens),
        format!(
            "{} tokens × {used} choices could reach at most {reachable} distinct experts in a layer{}",
            a.tokens,
            if reachable < experts { ": a longer prompt can reach more" } else { "" }
        ),
        format!(
            "A layer leans when one expert is chosen for {} of the tokens or more, over at least {LEANING_TOKENS} tokens (declared thresholds)",
            percent(LEANING_SHARE)
        ),
    ];
    figures.extend(leaning.iter().take(6).map(|(l, id, s)| format!("Layer {l}: expert {id} for {} of tokens", percent(*s))));
    out.push(Finding {
        id: "routing",
        topic: "Routing",
        basis: Basis::Observed,
        tone: if leaning.is_empty() { Tone::Plain } else { Tone::Notable },
        headline,
        figures,
        caveat: format!(
            "From {prompt}. A longer or different prompt routes differently, and near-ties in a router can fall either way \
             between the CPU and the card."
        ),
        // The expert banks of the layers that leaned, not their whole blocks.
        focus: (!leaning.is_empty()).then(|| Focus::Cells(leaning.iter().map(|l| (Some(l.0), "ffn_expert".to_string())).collect())),
    });
}

// ------------------------------------------------------------------ against another model

fn comparison(inputs: &Inputs, c: &MorphometryComparison, out: &mut Vec<Finding>) {
    if c.left_model != inputs.measured.model {
        return;
    }
    if !c.ok() {
        out.push(Finding {
            id: "difference",
            topic: "Against another",
            basis: Basis::Unmeasured,
            tone: Tone::Caution,
            headline: format!("It could not be compared with {}: {}", c.right_model, c.explanation),
            figures: c.unmeasured_sides.clone(),
            caveat: String::new(),
            focus: None,
        });
        return;
    }
    let both: Vec<_> = c.cells.iter().filter(|x| x.presence == CellPresence::Both).collect();
    let only_here = c.cells.iter().filter(|x| x.presence == CellPresence::LeftOnly).count();
    let only_there = c.cells.iter().filter(|x| x.presence == CellPresence::RightOnly).count();
    let sum = |f: fn(&model_anatomy::CellComparison) -> Option<i64>| {
        both.iter().map(|x| f(x)).try_fold(0i64, |a, v| v.and_then(|v| a.checked_add(v)))
    };
    let (left, delta) = (sum(|x| x.left_parameters), sum(|x| x.parameter_delta));
    let mut changed: Vec<_> = both.iter().filter(|x| x.parameter_delta.is_some_and(|d| d != 0)).collect();
    changed.sort_by_key(|x| std::cmp::Reverse(x.parameter_delta.map_or(0, i64::abs)));
    let mut headline = match (delta, left) {
        (Some(0), _) => format!("{} has the same parameter count as this model in every part both have", c.right_model),
        (Some(d), Some(l)) if l > 0 => format!(
            "{} has {} {} parameters than this model in the parts both have ({:+.1}%)",
            c.right_model,
            si(Some(d.abs())),
            if d > 0 { "more" } else { "fewer" },
            d as f64 / l as f64 * 100.0
        ),
        _ => format!("How {} differs from this model in parameters is UNMEASURED", c.right_model),
    };
    if only_here + only_there > 0 {
        headline.push_str(&format!("; {only_here} part{} only here, {only_there} only there", if only_here == 1 { "" } else { "s" }));
    }
    headline.push('.');
    let mut figures = vec![format!("{} parts in both", both.len())];
    figures.extend(changed.iter().take(6).map(|x| {
        format!("{}: {} parameters", inputs.place(x.block, &x.module), x.parameter_delta.map_or("-".to_string(), |d| format!("{d:+}")))
    }));
    figures.extend(c.gaps.iter().map(|g| format!("Gap: {g}")));
    out.push(Finding {
        id: "difference",
        topic: "Against another",
        basis: Basis::Counted,
        tone: if delta == Some(0) && only_here + only_there == 0 { Tone::Plain } else { Tone::Notable },
        headline,
        figures,
        caveat: "Structure only: counts and storage on the shared frame, nothing about quality or behaviour.".to_string(),
        focus: (!changed.is_empty()).then(|| Focus::Cells(changed.iter().take(12).map(|x| (x.block, x.module.clone())).collect())),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::morphometry::{measure_header, tests::{gguf_bytes, header}};
    use std::path::PathBuf;

    fn measured(tensors: &[(&str, &[u64], u32)], blocks: u32) -> Measured {
        let bytes = gguf_bytes(
            &[("general.architecture", Ok("llama")), ("llama.block_count", Err(blocks)), ("general.file_type", Err(15))],
            tensors,
        );
        measure_header(&header(&bytes), "tiny", PathBuf::from("tiny.gguf")).expect("measured")
    }

    fn labels() -> HashMap<String, String> {
        [("attn_q", "Query projection"), ("ffn_up", "Feed-forward up")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn cell(block: i64, module: &str, rms: f64, largest: f64, zeros: u64) -> serde_json::Value {
        serde_json::json!({"block": block, "module": module, "tensors": 1, "stats": {
            "count": 4096, "non_finite": 0, "zeros": zeros, "min": -largest, "max": largest, "sum": 0.0, "sum_abs": 0.0,
            "sum_sq": 0.0, "mean": 0.0, "std": rms, "rms": rms, "mean_abs": rms, "l2": null, "histogram": []}})
    }

    #[test]
    fn the_header_reads_as_size_work_layout_depth_and_precision_each_with_its_basis() {
        let mut tensors: Vec<(String, Vec<u64>, u32)> = vec![("token_embd.weight".into(), vec![8, 100], 12)];
        for b in 0..4 {
            tensors.push((format!("blk.{b}.attn_q.weight"), vec![8, 8], 12));
            tensors.push((format!("blk.{b}.ffn_up.weight"), vec![8, if b == 3 { 64 } else { 16 }], 12));
        }
        tensors.push(("output.weight".into(), vec![8, 100], 14));
        let borrowed: Vec<(&str, &[u64], u32)> = tensors.iter().map(|(n, d, t)| (n.as_str(), d.as_slice(), *t)).collect();
        let m = measured(&borrowed, 4);
        let labels = labels();
        let r = read(&Inputs { measured: &m, labels: &labels, weights: None, activations: None, comparison: None, weights_when: None, activations_when: None, prompt: None });
        let ids: Vec<&str> = r.findings.iter().map(|f| f.id).collect();
        assert_eq!(ids, ["size", "work", "layout", "depth", "precision"], "{:#?}", r.findings);
        let by = |id: &str| r.findings.iter().find(|f| f.id == id).unwrap();
        assert_eq!(by("size").basis, Basis::Counted);
        assert!(by("work").headline.starts_with("Dense"), "{}", by("work").headline);
        // Block 3's feed-forward is four times the others': the engine flags it, and the reading points at it.
        assert_eq!(by("depth").tone, Tone::Notable, "{}", by("depth").headline);
        assert_eq!(by("depth").focus, Some(Focus::Blocks(vec![3])));
        // Two formats (Q4_K and Q6_K): mixed, the exceptions lit.
        assert!(by("precision").headline.starts_with("Mixed precision"), "{}", by("precision").headline);
        assert!(by("precision").focus.as_ref().unwrap().covers(None, "output"));
        assert_eq!(r.further, [Further::Weights, Further::Activations, Further::Compare]);
        assert_eq!(r.glance.len(), 3, "size, work and the notable depth: {:?}", r.glance);
        assert!(r.glance[2].contains("built differently"), "{:?}", r.glance);
    }

    #[test]
    fn the_weights_flag_a_part_on_another_scale_heavy_extremes_and_many_zeros() {
        let m = measured(
            &[
                ("token_embd.weight", &[8, 100], 12),
                ("blk.0.attn_q.weight", &[8, 8], 12),
                ("blk.1.attn_q.weight", &[8, 8], 12),
                ("blk.2.attn_q.weight", &[8, 8], 12),
                ("blk.3.attn_q.weight", &[8, 8], 12),
                ("output.weight", &[8, 100], 12),
            ],
            4,
        );
        let w: WeightStatistics = serde_json::from_value(serde_json::json!({
            "path": "tiny.gguf", "refusal": null, "file_bytes": 1, "tensors": [], "refused": [], "complete": true,
            "cells": [cell(0, "attn_q", 0.02, 0.1, 0), cell(1, "attn_q", 0.021, 0.1, 0), cell(2, "attn_q", 0.019, 0.1, 0),
                      cell(3, "attn_q", 0.09, 6.0, 400)]}))
        .unwrap();
        let labels = labels();
        let r = read(&Inputs {
            measured: &m,
            labels: &labels,
            weights: Some(&w),
            activations: None,
            comparison: None,
            weights_when: Some("2 hours ago".into()),
            activations_when: None,
            prompt: None,
        });
        let by = |id: &str| r.findings.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("{id}: {:#?}", r.findings));
        assert_eq!(by("finite").tone, Tone::Plain);
        assert_eq!(by("zeros").figures[0], "Read from the file 2 hours ago", "when the weights were read heads each finding's figures");
        let scale = by("scale");
        assert_eq!(scale.tone, Tone::Notable);
        assert_eq!(scale.headline, "One kind of part changes scale with depth: Query projection in block 3 (up to 4.4× the usual).");
        assert_eq!(scale.focus, Some(Focus::Cells(vec![(Some(3), "attn_q".to_string())])));
        assert!(by("extremes").headline.contains("block 3"), "{}", by("extremes").headline);
        assert_eq!(by("extremes").tone, Tone::Notable, "6 / 0.09 is 67×");
        // 400 zeros of 16,384 weights: 2.4%.
        assert_eq!(by("zeros").tone, Tone::Notable, "{}", by("zeros").headline);
        assert!(!r.further.contains(&Further::Weights));
    }

    #[test]
    fn one_prompt_reads_as_signal_and_routing_and_says_it_is_one_prompt() {
        let m = measured(&[("token_embd.weight", &[8, 100], 12), ("blk.0.attn_q.weight", &[8, 8], 12), ("output.weight", &[8, 100], 12)], 1);
        let a = Activations::parse(
            r#"{"ok":true,"build":"b","architecture":"qwen3moe","layers":4,"expert_count":8,"tokens":10,"gpu_layers":0,
            "tensors":[{"name":"l_out-0","op":"l_out","layer":0,"nonfinite":0,"rms":1.0},
                       {"name":"l_out-1","op":"l_out","layer":1,"nonfinite":0,"rms":1.2},
                       {"name":"l_out-2","op":"l_out","layer":2,"nonfinite":0,"rms":6.0},
                       {"name":"l_out-3","op":"l_out","layer":3,"nonfinite":0,"rms":90.0}],
            "experts":[{"layer":0,"used_per_token":2,"counts":{"1":9,"2":5,"3":6}},
                       {"layer":1,"used_per_token":2,"counts":{"1":4,"2":4,"3":4,"4":4,"5":4}}]}"#,
        )
        .unwrap();
        let labels = labels();
        let r = read(&Inputs {
            measured: &m,
            labels: &labels,
            weights: None,
            activations: Some(&a),
            comparison: None,
            weights_when: None,
            activations_when: Some("yesterday".into()),
            prompt: Some("Hello there."),
        });
        let by = |id: &str| r.findings.iter().find(|f| f.id == id).unwrap_or_else(|| panic!("{id}: {:#?}", r.findings));
        let signal = by("signal");
        // Layer 3 is left out (computed only for the output token): 1.0 -> 6.0, the step at layer 2 (x5).
        assert!(signal.headline.contains("grows 6.0×") && signal.headline.contains("layer 2"), "{}", signal.headline);
        assert_eq!(signal.basis, Basis::Observed);
        assert_eq!(signal.focus, Some(Focus::Blocks(vec![2])));
        assert!(signal.caveat.contains("one prompt of 10 tokens"));
        assert_eq!(signal.figures[0], "Run yesterday, on the prompt \"Hello there.\"");
        let routing = by("routing");
        assert!(routing.headline.contains("used 4 of its 8 experts"), "{}", routing.headline);
        assert!(routing.headline.contains("layer 0: expert 1 for 90%"), "{}", routing.headline);
        assert_eq!(routing.focus, Some(Focus::Cells(vec![(Some(0), "ffn_expert".to_string())])));
        assert!(routing.figures[2].contains("at most 8 distinct"), "{:?}", routing.figures);
    }

    /// Run by hand: the reading of the model `LATTICE_DIGEST_MODEL` names, printed, with a saved weight statistics
    /// answer when `LATTICE_DIGEST_WEIGHTS` names one and a saved activation probe answer when
    /// `LATTICE_DIGEST_ACTIVATIONS` does.
    #[test]
    #[ignore = "reads a real model: run by hand with LATTICE_DIGEST_MODEL set"]
    fn a_real_model_is_read() {
        let path = PathBuf::from(std::env::var("LATTICE_DIGEST_MODEL").expect("LATTICE_DIGEST_MODEL"));
        let m = crate::lattice::morphometry::measure_file(&path, "model").expect("measured");
        let labels: HashMap<String, String> = [
            ("token_embd", "Token embedding"), ("attn_norm", "Attention norm"), ("attn_q_norm", "Query norm"),
            ("attn_k_norm", "Key norm"), ("ffn_norm", "Feed-forward norm"), ("output_norm", "Output norm"),
            ("attn_q", "Query projection"), ("attn_k", "Key projection"), ("attn_v", "Value projection"),
            ("attn_output", "Attention output"), ("ffn_gate", "Feed-forward gate"), ("ffn_up", "Feed-forward up"),
            ("ffn_down", "Feed-forward down"), ("ffn_expert", "Expert bank"), ("output", "Output projection"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let saved = |key: &str| std::env::var(key).ok().map(|p| std::fs::read_to_string(p).expect("a saved answer"));
        let weights: Option<WeightStatistics> = saved("LATTICE_DIGEST_WEIGHTS").map(|t| serde_json::from_str(&t).expect("weights"));
        let activations = saved("LATTICE_DIGEST_ACTIVATIONS").map(|t| Activations::parse(t.trim()).expect("parsed"));
        let r = read(&Inputs {
            measured: &m,
            labels: &labels,
            weights: weights.as_ref(),
            activations: activations.as_ref(),
            comparison: None,
            weights_when: None,
            activations_when: None,
            prompt: None,
        });
        println!("AT A GLANCE");
        for line in &r.glance {
            println!("  {line}");
        }
        for f in &r.findings {
            println!("
[{:?} · {}] {}: {}", f.tone, f.basis.word(), f.topic, f.headline);
            for line in &f.figures {
                println!("    • {line}");
            }
            println!("    not: {}", f.caveat);
            if let Some(focus) = &f.focus {
                let n = m.analysis.voxel_field.voxels.iter().filter(|v| focus.covers((v.x >= 0).then_some(v.x), &v.module)).count();
                println!("    lights {n} of {} cells", m.analysis.voxel_field.voxels.len());
            }
        }
    }

    #[test]
    fn small_words_read_as_people_say_them() {
        assert_eq!(runs(&[7, 0, 1, 2, 3, 9, 10]), "0-3, 7, 9-10");
        assert_eq!((percent(0.0004), percent(0.054), percent(0.92)), ("under 0.1%".into(), "5.4%".into(), "92%".into()));
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert!(Focus::Blocks(vec![2]).covers(Some(2), "anything") && !Focus::Blocks(vec![2]).covers(None, "output"));
    }
}
