//! The Morphometry tab's probes (weight statistics and an activation probe): the
//! two readings of a model that go past its header.
//!
//! - **Weight statistics** read every tensor's data from the GGUF file and dequantise it (the C++ engine:
//!   every GGML type bit for bit as gguf-py), streaming, never holding a tensor whole, and summarise each canonical
//!   (block, module) cell: count, mean, spread, RMS, extremes, exact zeros and non-finite values. The file is only
//!   read.
//! - **The activation probe** runs one forward pass of a prompt through the model on the pinned llama.cpp build, in a
//!   process of its own (`lattice-probe`, probe/ in this workspace), and records each layer's activations and, for a
//!   mixture-of-experts model, which experts its router chose for each token.
//!
//! Neither is automatic: each runs because a person pressed its button, off the window's thread, and can be stopped.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A weight statistics run in progress: bytes done and the total, and the stop flag.
#[derive(Default)]
pub struct Running {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub stop: AtomicBool,
}

impl Running {
    /// The share done, 0 to 1.
    pub fn share(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 { 0.0 } else { (self.done.load(Ordering::Relaxed) as f64 / total as f64) as f32 }
    }
}

/// Weight statistics of the GGUF file at `path`, reporting progress into `running` and stopping when its flag is set:
/// the engine's answer as it wrote it (kept whole by morphometry_store), and read.
pub fn weights(path: &Path, running: Arc<Running>) -> Result<(model_anatomy::WeightStatistics, String), String> {
    let report = |done: u64, total: u64| {
        running.done.store(done, Ordering::Relaxed);
        running.total.store(total, Ordering::Relaxed);
    };
    let text = model_anatomy::weight_statistics_json(path, 0, Some(&report), Some(&running.stop)).map_err(|e| e.to_string())?;
    let read = parse_weights(&text)?;
    Ok((read, text))
}

/// The engine's weight statistics answer, read.
pub fn parse_weights(text: &str) -> Result<model_anatomy::WeightStatistics, String> {
    serde_json::from_str(text).map_err(|e| format!("the weight statistics answer could not be read: {e}"))
}

/// What the activation probe wrote (probe/src/probe.cpp's JSON), read field by field.
#[derive(Clone, Debug, Default)]
pub struct Activations {
    pub ok: bool,
    pub refusal: Option<String>,
    pub build: String,
    pub architecture: String,
    pub layers: i64,
    pub expert_count: Option<i64>,
    pub tokens: i64,
    pub gpu_layers: i64,
    pub load_ms: Option<f64>,
    pub decode_ms: Option<f64>,
    pub tensors: Vec<Activation>,
    pub experts: Vec<Routing>,
}

/// One recorded activation tensor.
#[derive(Clone, Debug, Default)]
pub struct Activation {
    pub name: String,
    pub op: String,
    pub layer: i64,
    pub nonfinite: i64,
    pub skipped: Option<String>,
    pub rms: Option<f64>,
}

/// One MoE layer's router choices: how many times each expert was chosen over the prompt's tokens.
#[derive(Clone, Debug, Default)]
pub struct Routing {
    pub layer: i64,
    pub used_per_token: i64,
    pub counts: std::collections::BTreeMap<String, i64>,
}

fn text(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
}

fn int(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(serde_json::Value::as_i64)
}

fn float(v: &serde_json::Value, key: &str) -> Option<f64> {
    v.get(key).and_then(serde_json::Value::as_f64)
}

impl Activations {
    /// The probe's JSON answer; None when it is not an object.
    pub fn parse(answer: &str) -> Option<Activations> {
        let v: serde_json::Value = serde_json::from_str(answer).ok()?;
        v.as_object()?;
        let list = |key: &str| v.get(key).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
        Some(Activations {
            ok: v.get("ok").and_then(serde_json::Value::as_bool).unwrap_or(false),
            refusal: v.get("refusal").and_then(serde_json::Value::as_str).map(str::to_string),
            build: text(&v, "build"),
            architecture: text(&v, "architecture"),
            layers: int(&v, "layers").unwrap_or(0),
            expert_count: int(&v, "expert_count"),
            tokens: int(&v, "tokens").unwrap_or(0),
            gpu_layers: int(&v, "gpu_layers").unwrap_or(0),
            load_ms: float(&v, "load_ms"),
            decode_ms: float(&v, "decode_ms"),
            tensors: list("tensors")
                .iter()
                .map(|t| Activation {
                    name: text(t, "name"),
                    op: text(t, "op"),
                    layer: int(t, "layer").unwrap_or(-1),
                    nonfinite: int(t, "nonfinite").unwrap_or(0),
                    skipped: t.get("skipped").and_then(serde_json::Value::as_str).map(str::to_string),
                    rms: float(t, "rms"),
                })
                .collect(),
            experts: list("experts")
                .iter()
                .map(|e| Routing {
                    layer: int(e, "layer").unwrap_or(-1),
                    used_per_token: int(e, "used_per_token").unwrap_or(0),
                    counts: e
                        .get("counts")
                        .and_then(serde_json::Value::as_object)
                        .map(|m| m.iter().filter_map(|(k, c)| Some((k.clone(), c.as_i64()?))).collect())
                        .unwrap_or_default(),
                })
                .collect(),
        })
    }
}

impl Activations {
    /// The activation `op` of `layer`, when it was recorded.
    pub fn at(&self, op: &str, layer: i64) -> Option<&Activation> {
        self.tensors.iter().find(|t| t.op == op && t.layer == layer)
    }
}

/// The probe's program: `LATTICE_PROBE` when it names a file, else `lattice-probe.exe` beside this window's own
/// executable (where a build of this workspace puts it).
pub fn helper() -> Option<PathBuf> {
    let path = match std::env::var_os("LATTICE_PROBE") {
        Some(named) => PathBuf::from(named),
        None => std::env::current_exe().ok()?.with_file_name("lattice-probe.exe"),
    };
    path.is_file().then_some(path)
}

/// The folder of the llama.cpp build the probe loads: the managed server's binary's folder (ALELYON_LLAMA_SERVER,
/// else the pinned install), as llama::bench finds it.
pub fn build_folder() -> Result<PathBuf, String> {
    let paths = lattice_core::llama::files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
    let binary = lattice_core::llama::files::find_binary(&paths).map_err(|why| format!("no llama.cpp build: {why:?}"))?;
    binary.parent().map(Path::to_path_buf).ok_or_else(|| "the llama.cpp build has no folder".to_string())
}

/// Run the activation probe on `model` with `prompt`, `gpu_layers` layers on the card; `stop` ends it early. The
/// probe's answer as it wrote it (kept whole by morphometry_store), and read.
pub fn activations(model: &Path, prompt: &str, gpu_layers: u32, stop: Arc<AtomicBool>) -> Result<(Activations, String), String> {
    let helper = helper().ok_or("the activation probe (lattice-probe.exe) is not beside this program")?;
    let folder = build_folder()?;
    let prompt_file = std::env::temp_dir().join(format!("lattice-probe-{}.txt", std::process::id()));
    std::fs::write(&prompt_file, prompt).map_err(|e| format!("the prompt could not be written for the probe: {e}"))?;
    let mut command = std::process::Command::new(&helper);
    command
        .arg("--dir")
        .arg(&folder)
        .arg("--model")
        .arg(model)
        .arg("--prompt-file")
        .arg(&prompt_file)
        .arg("--gpu-layers")
        .arg(gpu_layers.to_string())
        .arg("--ctx")
        .arg("2048")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|e| format!("the activation probe did not start: {e}"));
    let result = (|| {
        let child = child.as_mut().map_err(|e| e.clone())?;
        let mut stdout = child.stdout.take().ok_or("the probe has no output")?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut stdout, &mut text).map(|_| text)
        });
        loop {
            if stop.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Stopped.".to_string());
            }
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let text = reader.join().map_err(|_| "the probe's output was lost")?.map_err(|e| e.to_string())?;
        let found = Activations::parse(text.trim()).ok_or("the probe's answer could not be read")?;
        match (&found.ok, &found.refusal) {
            (false, Some(why)) => Err(format!("The probe refused: {why}.")),
            (false, None) => Err("The probe refused without saying why.".to_string()),
            _ => Ok((found, text.trim().to_string())),
        }
    })();
    let _ = std::fs::remove_file(&prompt_file);
    result
}

/// What llama.cpp needs on the card besides the layers' weights (its compute buffers and the cache at the probe's
/// 2,048-token context): 1.5 GiB, chosen from a measured run (qwen3-coder-30b at 512 tokens took a 548 MiB compute
/// buffer) with room for a longer prompt. A DECLARED margin, not a measurement of any other model.
pub const CARD_MARGIN: u64 = 3 << 29;

/// How many of a model's `layers` should fit on a card with `free` bytes free, the layers taken as equal shares of the
/// file's `bytes` (a DERIVED estimate: a layer's real size varies, and the output and embedding tensors are counted in
/// the shares). Never more than the model has.
pub fn suggested_layers(free: u64, bytes: u64, layers: u64) -> u64 {
    if bytes == 0 || layers == 0 {
        return 0;
    }
    let usable = free.saturating_sub(CARD_MARGIN) as u128;
    ((usable * layers as u128 / bytes as u128) as u64).min(layers)
}

/// The card's free memory now, from the engine's probe (its own counter; no device opened).
pub fn free_card_memory() -> Result<u64, String> {
    let reading = model_anatomy::probe().map_err(|e| format!("the machine probe failed: {e}"))?;
    reading
        .gpu
        .and_then(|g| g.free_bytes)
        .and_then(|b| u64::try_from(b).ok())
        .ok_or_else(|| "the card's free memory is UNMEASURED (VK_LOADER_DEVICE_ID_FILTER names no adapter)".to_string())
}

/// The experts a layer's router chose most, as "id ×count", most chosen first.
pub fn top_experts(routing: &Routing, n: usize) -> String {
    let mut counts: Vec<(&String, &i64)> = routing.counts.iter().collect();
    counts.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.parse::<i64>().unwrap_or(0).cmp(&b.0.parse::<i64>().unwrap_or(0))));
    counts.iter().take(n).map(|(id, count)| format!("{id} ×{count}")).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run by hand: the whole activation probe path on the model `LATTICE_PROBE_MODEL` names, on the CPU, with the
    /// helper `LATTICE_PROBE` names and the build ALELYON_LLAMA_SERVER names.
    #[test]
    #[ignore = "loads a real model: run by hand with LATTICE_PROBE and LATTICE_PROBE_MODEL set"]
    fn a_real_model_is_probed() {
        let model = PathBuf::from(std::env::var("LATTICE_PROBE_MODEL").expect("LATTICE_PROBE_MODEL"));
        let stop = Arc::new(AtomicBool::new(false));
        let (a, _) = activations(&model, "The capital of France is Paris.", 0, stop).unwrap_or_else(|why| panic!("{why}"));
        println!("{} layers {} tokens {} pass {:?} ms", a.architecture, a.layers, a.tokens, a.decode_ms);
        assert!(a.ok && a.layers > 0 && (0..a.layers).all(|l| a.at("l_out", l).is_some()));
        if a.expert_count.is_some() {
            assert_eq!(a.experts.len() as i64, a.layers, "every MoE layer's choices");
        }
    }

    #[test]
    fn the_suggestion_leaves_the_margin_and_never_exceeds_the_model() {
        const GIB: u64 = 1 << 30;
        // qwen3-coder-30b: 15.39 GB over 48 layers; 14.25 GiB free less 1.5 GiB is 13.69 GB, 42.7 layers: 42.
        assert_eq!(suggested_layers(14 * GIB + GIB / 4, 15_386_884_896, 48), 42);
        assert_eq!(suggested_layers(GIB, 15_386_884_896, 48), 0, "less than the margin: none");
        assert_eq!(suggested_layers(64 * GIB, 1_000_000, 32), 32, "a small model: all of it");
        assert_eq!((suggested_layers(8 * GIB, 0, 4), suggested_layers(8 * GIB, 10, 0)), (0, 0));
    }

    #[test]
    fn the_probes_answer_is_read_and_a_refusal_says_why() {
        let text = r#"{"ok":true,"build":"llama.cpp fb27a52","architecture":"qwen3moe","layers":2,"expert_count":4,
            "tokens":3,"token_ids":[1,2,3],"gpu_layers":0,"context":512,"load_ms":1.5,"decode_ms":2.5,
            "tensors":[{"name":"l_out-0","op":"l_out","layer":0,"type":"f32","shape":[8,3,1,1],"times":1,"count":24,
            "nonfinite":0,"zeros":0,"min":-1,"max":2,"mean":0.1,"std":0.5,"rms":0.6,"mean_abs":0.4},
            {"name":"ffn_moe_topk-0","op":"ffn_moe_topk","layer":0,"type":"other","shape":[2,3,1,1],"times":1,"count":0,
            "nonfinite":0,"zeros":0,"skipped":"its type is not F32, F16 or I32"}],
            "experts":[{"layer":0,"used_per_token":2,"counts":{"1":3,"3":2,"0":1}}]}"#;
        let a = Activations::parse(text).unwrap();
        assert!(a.ok && a.architecture == "qwen3moe" && a.expert_count == Some(4));
        assert_eq!(a.at("l_out", 0).and_then(|t| t.rms), Some(0.6));
        assert!(a.at("ffn_moe_topk", 0).unwrap().skipped.is_some());
        assert_eq!(top_experts(&a.experts[0], 2), "1 ×3, 3 ×2");
        let refused = Activations::parse(r#"{"ok":false,"refusal":"llama.dll is not the pinned build"}"#).unwrap();
        assert!(!refused.ok);
        assert!(Activations::parse("[1]").is_none() && Activations::parse("not json").is_none());
        assert_eq!(refused.refusal.as_deref(), Some("llama.dll is not the pinned build"));
    }
}
