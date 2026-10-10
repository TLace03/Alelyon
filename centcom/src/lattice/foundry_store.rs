//! What the Foundry tab keeps between runs, in `<globals>/lattice_native/foundry.json`: the person's cost declarations
//! (as typed, with their sources and dates) and each model's last benchmark (its rates, the CPU package's energy,
//! when it was measured, and the model file's size and modification time).
//!
//! A benchmark is a reading of one file at one time: when the file it measured has changed (another size or another
//! modification time) or is gone, the reading is kept but marked stale, and a cost is never computed from it. The file
//! is rewritten in place (the same file, so no new file is created per save), off the window's thread; one that is
//! missing is an empty record, and one that cannot be read is named and replaced only by the next save.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::foundry::{Bench, Costs, Hosted};

/// The record's own version.
pub const SCHEMA: i64 = 1;

/// Where the record lives.
pub fn path(state: &lattice_core::StateRoot) -> PathBuf {
    state.globals.join("lattice_native").join("foundry.json")
}

/// What was kept.
#[derive(Clone, Debug, Default)]
pub struct Kept {
    pub costs: Costs,
    pub benchmarks: BTreeMap<String, Bench>,
}

/// A model file's size and modification time (seconds since 1970), as a benchmark records it.
pub fn file_stamp(path: &Path) -> Option<(u64, Option<f64>)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64());
    Some((meta.len(), modified))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn float(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn hosted_value(h: &Hosted) -> Value {
    json!({"provider": h.provider, "model": h.model, "input": h.input, "output": h.output, "source": h.source, "as_of": h.as_of})
}

fn hosted_of(v: &Value) -> Hosted {
    Hosted {
        provider: text(v, "provider"),
        model: text(v, "model"),
        input: text(v, "input"),
        output: text(v, "output"),
        source: text(v, "source"),
        as_of: text(v, "as_of"),
    }
}

fn bench_value(b: &Bench) -> Value {
    let t = &b.throughput;
    json!({
        "model": t.model, "context": t.context,
        "decode_tokens_per_second": t.decode_tokens_per_second, "prefill_tokens_per_second": t.prefill_tokens_per_second,
        "method": t.method, "provenance": t.provenance,
        "prompt_tokens": b.prompt_tokens, "load_seconds": b.load_seconds, "placement": b.placement,
        "cpu_joules": b.cpu_joules, "cpu_watts": b.cpu_watts,
        "measured_at": b.measured_at, "model_bytes": b.model_bytes, "model_modified": b.model_modified,
    })
}

fn bench_of(v: &Value) -> Option<Bench> {
    let throughput = model_anatomy::ThroughputReading {
        method: text(v, "method"),
        provenance: text(v, "provenance"),
        ..model_anatomy::ThroughputReading::new(
            &text(v, "model"),
            v.get("context").and_then(Value::as_i64)?,
            float(v, "decode_tokens_per_second"),
            float(v, "prefill_tokens_per_second"),
        )
    };
    Some(Bench {
        throughput,
        prompt_tokens: v.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
        load_seconds: float(v, "load_seconds")?,
        placement: text(v, "placement"),
        cpu_joules: float(v, "cpu_joules"),
        cpu_watts: float(v, "cpu_watts"),
        measured_at: float(v, "measured_at")?,
        model_bytes: v.get("model_bytes").and_then(Value::as_u64)?,
        model_modified: float(v, "model_modified"),
        stale: false,
    })
}

/// The record as JSON text.
pub fn to_text(kept: &Kept) -> String {
    let c = &kept.costs;
    let value = json!({
        "schema": SCHEMA,
        "costs": {
            "gpu_watts": c.gpu_watts, "gpu_source": c.gpu_source, "usd_per_kwh": c.usd_per_kwh,
            "tariff_source": c.tariff_source, "tariff_as_of": c.tariff_as_of,
            "input_tokens": c.input_tokens, "output_tokens": c.output_tokens,
            "hosted": c.hosted.iter().map(hosted_value).collect::<Vec<_>>(),
        },
        "benchmarks": kept.benchmarks.iter().map(|(k, b)| (k.clone(), bench_value(b))).collect::<serde_json::Map<_, _>>(),
    });
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

/// The record from JSON text, with each benchmark marked stale where `current` (a model name's file stamp now, None
/// when the file is gone) differs from the one it measured. Err names what is wrong with a record that cannot be read.
pub fn from_text(text_in: &str, current: impl Fn(&str) -> Option<(u64, Option<f64>)>) -> Result<Kept, String> {
    let v: Value = serde_json::from_str(text_in).map_err(|e| format!("it is not JSON ({e})"))?;
    if v.get("schema").and_then(Value::as_i64) != Some(SCHEMA) {
        return Err(format!("its schema is not {SCHEMA}"));
    }
    let c = v.get("costs").cloned().unwrap_or(Value::Null);
    let costs = Costs {
        gpu_watts: text(&c, "gpu_watts"),
        gpu_source: text(&c, "gpu_source"),
        usd_per_kwh: text(&c, "usd_per_kwh"),
        tariff_source: text(&c, "tariff_source"),
        tariff_as_of: text(&c, "tariff_as_of"),
        input_tokens: text(&c, "input_tokens"),
        output_tokens: text(&c, "output_tokens"),
        hosted: c.get("hosted").and_then(Value::as_array).map(|a| a.iter().map(hosted_of).collect()).unwrap_or_default(),
        draft: Hosted::default(),
    };
    let mut benchmarks = BTreeMap::new();
    if let Some(map) = v.get("benchmarks").and_then(Value::as_object) {
        for (name, b) in map {
            if let Some(mut bench) = bench_of(b) {
                bench.stale = current(name) != Some((bench.model_bytes, bench.model_modified));
                benchmarks.insert(name.clone(), bench);
            }
        }
    }
    Ok(Kept { costs, benchmarks })
}

/// Read the record: a missing file is an empty record; one that cannot be read is named.
pub fn load(state: &lattice_core::StateRoot) -> Result<Kept, String> {
    let file = path(state);
    let text_in = match std::fs::read_to_string(&file) {
        Ok(text_in) => text_in,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Kept::default()),
        Err(e) => return Err(format!("{} could not be read: {e}", file.display())),
    };
    let paths = lattice_core::llama::files::LlamaPaths::from_env(&lattice_core::ProcessEnv);
    let models = lattice_core::llama::files::list_models(&paths.models_dir);
    let current = |name: &str| models.iter().find(|m| m.name == name).and_then(|m| file_stamp(&m.path));
    from_text(&text_in, current).map_err(|why| format!("{} was not read: {why}; the next save replaces it", file.display()))
}

/// Write the record in place.
pub fn save(state: &lattice_core::StateRoot, kept: &Kept) -> Result<(), String> {
    let file = path(state);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
    }
    std::fs::write(&file, to_text(kept)).map_err(|e| format!("{} could not be written: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench(model: &str) -> Bench {
        Bench {
            throughput: model_anatomy::ThroughputReading {
                method: "one generation of up to 128 tokens".into(),
                ..model_anatomy::ThroughputReading::new(model, 4096, Some(91.9), Some(106.7))
            },
            prompt_tokens: 8,
            load_seconds: 13.0,
            placement: "--fit on".into(),
            cpu_joules: Some(211.0),
            cpu_watts: None,
            measured_at: 1_791_400_000.5,
            model_bytes: 15_386_884_896,
            model_modified: Some(1_791_300_000.25),
            stale: false,
        }
    }

    #[test]
    fn what_is_kept_reads_back_and_a_changed_model_file_makes_its_benchmark_stale() {
        let mut kept = Kept::default();
        kept.costs.gpu_watts = "304".into();
        kept.costs.tariff_source = "my bill, \"October\"".into();
        kept.costs.hosted.push(Hosted { provider: "P".into(), model: "M".into(), input: "3".into(), ..Hosted::default() });
        kept.benchmarks.insert("a".into(), bench("a"));
        kept.benchmarks.insert("b".into(), bench("b"));
        let text_out = to_text(&kept);
        let same = Some((15_386_884_896, Some(1_791_300_000.25)));
        let back = from_text(&text_out, |name| if name == "a" { same } else { Some((1, None)) }).unwrap();
        assert_eq!(back.costs.gpu_watts, "304");
        assert_eq!(back.costs.tariff_source, "my bill, \"October\"");
        assert_eq!(back.costs.hosted, kept.costs.hosted);
        let a = &back.benchmarks["a"];
        assert!(!a.stale, "the same file: current");
        assert_eq!((a.throughput.decode_tokens_per_second, a.cpu_watts, a.measured_at), (Some(91.9), None, 1_791_400_000.5));
        assert!(back.benchmarks["b"].stale, "another file under the name: stale");
        let gone = from_text(&text_out, |_| None).unwrap();
        assert!(gone.benchmarks.values().all(|b| b.stale), "a missing file: stale");
        assert!(from_text("{", |_| None).unwrap_err().contains("not JSON"));
        assert!(from_text(r#"{"schema": 2}"#, |_| None).unwrap_err().contains("schema"));
    }
}
