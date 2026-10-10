//! Voice enrolment: Sinai's voiceprint of its person, shown, made again, or erased.
//!
//! The voiceprint gates authority by voice: a spoken command that would grant authority is accepted only in the
//! enrolled voice (`CHARTER.md`). It is made by a registered probe (the speaker probe), and this
//! window runs that probe's own two scripts rather than a copy of them:
//! - `record_owner.py` shows 24 sentences one at a time and records each from the default microphone, stopped by Sinai's
//!   own end-of-speech rule; the clips go to `speaker/owner/` in Sinai's state home, on this PC only;
//! - `threshold_probe.py` makes the voiceprint from the first 8, scores the other 16 against it and against 54
//!   synthetic voices reading the same sentences, and sets the threshold by the rule registered beforehand. Only when
//!   both registered criteria pass (at least 15 of 16 of the person's trials accepted, none of the 864 others) does it
//!   write `voiceprint.json` and `decision.json`; a failed run writes neither, and the voiceprint before it stays.
//!
//! What this window adds is care around them. Before recording, the clips already there are moved aside into
//! `speaker/previous-<time>/` (never deleted); erasing moves the voiceprint, its decision and the clips aside into
//! `speaker/erased-<time>/` (asked twice; any voice can then grant authority, as with no voiceprint).
//! Sinai's loop reads the voiceprint when it starts, so a change takes effect at its next start.
//!
//! The scripts run under Angel's runtime interpreter, the one with onnxruntime, sounddevice and Kokoro. Where the
//! scripts, that interpreter, the models and Sinai's state home are is the build's to say (`installed.rs`, the `voice`
//! plug-in, the official build's `voice.rs`); the public build carries none, and its enrolment card says so.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;

use serde_json::Value;

/// How many sentences are read (`record_owner.py`; the first 8 make the voiceprint).
pub const SENTENCES: usize = 24;
/// The speaker model's file name (`speaker.py`), for this module's tests; the build that finds the model names it
/// itself.
#[cfg(test)]
pub const MODEL_FILE: &str = "wespeaker_en_voxceleb_resnet34.onnx";

/// Where everything is.
#[derive(Clone, Debug, PartialEq)]
pub struct Places {
    /// Sinai's state home, as `state_home.py` finds it: the folder of `ANGEL_MEMORY_DB` when that is set, else
    /// `~/.alelyon/angel`.
    pub home: PathBuf,
    pub python: PathBuf,
    /// The probe's folder in the repository's main checkout.
    pub scripts: PathBuf,
    /// Kokoro's model and voices, which make the synthetic voices (`CENTCOM_ANGEL_MODELS`, else Angel's install).
    pub models: PathBuf,
    pub speaker_model: PathBuf,
}

impl From<alelyon_plugin_api::VoiceTools> for Places {
    fn from(t: alelyon_plugin_api::VoiceTools) -> Places {
        Places { home: t.home, python: t.python, scripts: t.scripts, models: t.models, speaker_model: t.speaker_model }
    }
}

impl Places {
    /// Where everything is, as the build finds it: an error in words when this build has no voice enrolment or a
    /// place was not found.
    pub fn find() -> Result<Places, String> {
        match crate::installed::voice() {
            None => Err(crate::installed::VOICE_NOT_IN_BUILD.to_string()),
            Some(found) => found.map(Places::from),
        }
    }

    pub fn speaker(&self) -> PathBuf {
        self.home.join("speaker")
    }

    /// What is missing for a re-enrolment, in words; empty when nothing is.
    pub fn missing(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.python.is_file() {
            out.push(format!("Angel's runtime interpreter is not at {}", self.python.display()));
        }
        for script in ["record_owner.py", "threshold_probe.py"] {
            if !self.scripts.join(script).is_file() {
                out.push(format!("the probe's {script} is not in {}", self.scripts.display()));
            }
        }
        for file in ["kokoro-v1.0.onnx", "voices-v1.0.bin"] {
            if !self.models.join(file).is_file() {
                out.push(format!("{file} is not in {}", self.models.display()));
            }
        }
        if !self.speaker_model.is_file() {
            out.push(format!("the speaker model is not at {}", self.speaker_model.display()));
        }
        out
    }
}

/// The enrolment as its files say.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Enrolment {
    pub threshold: Option<f64>,
    pub date: String,
    pub measured_by: String,
    pub model_sha256: String,
    /// How many recordings made the voiceprint, when, and its note.
    pub utterances: Option<u64>,
    pub created: String,
    pub clips: usize,
    pub run: Option<Run>,
    /// Folders set aside earlier (erased, or the clips before a re-enrolment), newest first.
    pub aside: Vec<String>,
    /// What could not be read.
    pub problems: Vec<String>,
}

impl Enrolment {
    pub fn enrolled(&self) -> bool {
        self.threshold.is_some() && self.utterances.is_some()
    }
}

/// A run of the threshold probe, as its results file says.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Run {
    pub accepted_genuine: u64,
    pub genuine: usize,
    pub accepted_impostor: u64,
    pub impostor_trials: u64,
    pub threshold: f64,
    pub separated: bool,
    pub passed: bool,
}

impl Run {
    pub fn from_json(v: &Value) -> Option<Run> {
        let verdicts = v.get("verdicts")?;
        Some(Run {
            accepted_genuine: v.get("accepted_genuine")?.as_u64()?,
            genuine: v.get("genuine")?.as_array()?.len(),
            accepted_impostor: v.get("accepted_impostor")?.as_u64()?,
            impostor_trials: v.get("impostor_trials")?.as_u64()?,
            threshold: v.get("threshold")?.as_f64()?,
            separated: v.get("separated").and_then(Value::as_bool).unwrap_or(false),
            passed: verdicts.get("B1").and_then(Value::as_bool) == Some(true) && verdicts.get("B2").and_then(Value::as_bool) == Some(true),
        })
    }

    pub fn words(&self) -> String {
        format!(
            "{} of {} of your trials accepted, {} of {} synthetic-voice trials accepted, at a threshold of {:.4}: {}",
            self.accepted_genuine,
            self.genuine,
            self.accepted_impostor,
            self.impostor_trials,
            self.threshold,
            if self.passed { "passed" } else { "did not pass (nothing was written)" }
        )
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Read the enrolment in `places` (the vector itself is not kept: only how it was made).
pub fn read(places: &Places) -> Enrolment {
    let speaker = places.speaker();
    let mut e = Enrolment::default();
    match read_json(&speaker.join("decision.json")) {
        Ok(d) => {
            e.threshold = d.get("threshold").and_then(Value::as_f64);
            e.date = d.get("date").and_then(Value::as_str).unwrap_or_default().to_string();
            e.measured_by = d.get("measured_by").and_then(Value::as_str).unwrap_or_default().to_string();
            e.model_sha256 = d.get("model_sha256").and_then(Value::as_str).unwrap_or_default().to_string();
        }
        Err(_) if !speaker.join("decision.json").exists() => {}
        Err(why) => e.problems.push(format!("decision.json could not be read: {why}")),
    }
    match read_json(&speaker.join("voiceprint.json")) {
        Ok(v) => {
            e.utterances = v.get("utterances").and_then(Value::as_u64);
            e.created = v.get("created").and_then(Value::as_str).unwrap_or_default().to_string();
        }
        Err(_) if !speaker.join("voiceprint.json").exists() => {}
        Err(why) => e.problems.push(format!("voiceprint.json could not be read: {why}")),
    }
    e.clips = fs::read_dir(speaker.join("owner"))
        .map(|d| d.flatten().filter(|f| f.path().extension().and_then(|x| x.to_str()) == Some("wav")).count())
        .unwrap_or(0);
    if !e.measured_by.is_empty() {
        // the probe records the path it was given: absolute, or relative to its own folder
        let given = PathBuf::from(&e.measured_by);
        let path = if given.is_absolute() { given } else { places.scripts.join(given) };
        match read_json(&path) {
            Ok(v) => e.run = Run::from_json(&v),
            Err(why) => e.problems.push(format!("the run it was measured by ({}) could not be read: {why}", path.display())),
        }
    }
    let mut aside: Vec<String> = fs::read_dir(&speaker)
        .map(|d| {
            d.flatten()
                .filter(|f| f.path().is_dir())
                .map(|f| f.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("erased-") || n.starts_with("previous-"))
                .collect()
        })
        .unwrap_or_default();
    aside.sort_by(|a, b| b.split_once('-').map(|x| x.1).cmp(&a.split_once('-').map(|x| x.1)));
    e.aside = aside;
    e
}

/// A UTC stamp for a folder name: `20261007T120000Z`.
pub fn stamp(now: f64) -> String {
    // as the Fleet page names a cleared guard marker
    crate::utc::full(now).replace(['-', ':'], "").replace(' ', "T")
}

/// Move what is there aside into `speaker/<prefix>-<stamp>/`, keeping each name: never deleted. Returns the folder, or
/// None when there was nothing to move.
pub fn set_aside(speaker: &Path, names: &[&str], prefix: &str, now: f64) -> Result<Option<PathBuf>, String> {
    let present: Vec<&&str> = names.iter().filter(|n| speaker.join(n).exists()).collect();
    if present.is_empty() {
        return Ok(None);
    }
    let dest = speaker.join(format!("{prefix}-{}", stamp(now)));
    if dest.exists() {
        return Err(format!("{} is already there", dest.display()));
    }
    fs::create_dir_all(&dest).map_err(|e| format!("{} could not be made: {e}", dest.display()))?;
    for name in present {
        fs::rename(speaker.join(name), dest.join(name)).map_err(|e| format!("{name} could not be moved aside: {e}"))?;
    }
    Ok(Some(dest))
}

/// Erase: the voiceprint, its decision and the clips moved aside together.
pub fn erase(places: &Places, now: f64) -> Result<Option<PathBuf>, String> {
    set_aside(&places.speaker(), &["voiceprint.json", "decision.json", "owner"], "erased", now)
}

// ------------------------------------------------------------------- the recorder, driven

/// What the recorder said, line by line.
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    /// Sentence `index` (1-based) of `total`, for the voiceprint or a trial, waiting for Enter.
    Prompt {
        index: usize,
        total: usize,
        voiceprint: bool,
        sentence: String,
    },
    /// The clip was kept, this long.
    Saved(f64),
    /// Too short to keep: the same sentence again.
    TooShort(f64),
    Done,
}

/// Reads the recorder's output a line at a time (`record_owner.py`'s own words).
#[derive(Default)]
pub struct Reader {
    pending: Option<(usize, usize, bool)>,
}

impl Reader {
    pub fn line(&mut self, line: &str) -> Option<Said> {
        let t = line.trim();
        if let Some((index, total, voiceprint)) = self.pending {
            if t.is_empty() {
                return None;
            }
            self.pending = None;
            return Some(Said::Prompt { index, total, voiceprint, sentence: t.to_string() });
        }
        if let Some(rest) = t.strip_prefix('(') {
            // "(3/24, voiceprint) Press Enter, then say:"
            if let Some((head, tail)) = rest.split_once(')') {
                if tail.trim_start().starts_with("Press Enter") {
                    let (count, part) = head.split_once(", ").unwrap_or((head, ""));
                    let (i, n) = count.split_once('/')?;
                    self.pending = Some((i.trim().parse().ok()?, n.trim().parse().ok()?, part.trim() == "voiceprint"));
                }
            }
            return None;
        }
        let seconds = |s: &str| s.split_whitespace().next().and_then(|n| n.parse::<f64>().ok());
        if let Some(rest) = t.strip_prefix("saved ") {
            return seconds(rest).map(Said::Saved);
        }
        if let Some(rest) = t.strip_prefix("heard only ") {
            return seconds(rest).map(Said::TooShort);
        }
        t.starts_with("Done:").then_some(Said::Done)
    }
}

/// A running script: what it says arrives on `said`, line by line, and it ends with its exit.
pub struct Script {
    child: Child,
    stdin: Option<ChildStdin>,
    _job: Option<crate::job::Job>,
    pub lines: mpsc::Receiver<String>,
}

impl Script {
    fn start(places: &Places, script: &str, args: &[String]) -> Result<Script, String> {
        let mut command = Command::new(&places.python);
        command
            .arg("-u")
            .arg(places.scripts.join(script))
            .args(args)
            .current_dir(&places.scripts)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONIOENCODING", "utf-8");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn().map_err(|e| format!("{script} did not start: {e}"))?;
        // tied to this window: if Alelyon goes, the script goes with it (and the microphone closes)
        let job = crate::job::Job::new().ok().filter(|j| j.adopt(&child, script).is_ok());
        let (tx, rx) = mpsc::channel();
        for pipe in
            [child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|p| Box::new(p) as _)]
                .into_iter()
                .flatten()
        {
            let tx = tx.clone();
            let _ = std::thread::Builder::new().name("centcom-voice".into()).spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        let stdin = child.stdin.take();
        Ok(Script { child, stdin, _job: job, lines: rx })
    }

    /// The recorder: the sentences, a clip for each Enter.
    pub fn record(places: &Places) -> Result<Script, String> {
        Script::start(places, "record_owner.py", &[])
    }

    /// The registered scoring, its results written to `out`.
    pub fn score(places: &Places, out: &Path) -> Result<Script, String> {
        Script::start(
            places,
            "threshold_probe.py",
            &[
                "--models".into(),
                places.models.to_string_lossy().into_owned(),
                "--speaker-model".into(),
                places.speaker_model.to_string_lossy().into_owned(),
                "--out".into(),
                out.to_string_lossy().into_owned(),
            ],
        )
    }

    /// Press Enter: the recorder starts listening for the sentence on show.
    pub fn enter(&mut self) -> bool {
        self.stdin.as_mut().is_some_and(|s| s.write_all(b"\n").and_then(|_| s.flush()).is_ok())
    }

    /// The exit code, once it has ended.
    pub fn ended(&mut self) -> Option<Option<i32>> {
        self.child.try_wait().ok().flatten().map(|status| status.code())
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Script {
    fn drop(&mut self) {
        if self.ended().is_none() {
            self.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn places(root: &Path) -> Places {
        Places {
            home: root.to_path_buf(),
            python: root.join("python.exe"),
            scripts: root.join("scripts"),
            models: root.join("models"),
            speaker_model: root.join("models").join(MODEL_FILE),
        }
    }

    #[test]
    fn the_recorders_own_words_are_read() {
        let mut r = Reader::default();
        let out: Vec<Said> = [
            "Speaker probe, part B: record the speaker's voice",
            "Clips go to C:\\x\\speaker\\owner",
            "(3/24, voiceprint) Press Enter, then say:",
            "",
            "    I would like a cup of coffee before the meeting starts at nine.",
            "  heard only 0.4 s; let us try that one again.",
            "(3/24, voiceprint) Press Enter, then say:",
            "",
            "    I would like a cup of coffee before the meeting starts at nine.",
            "  saved 3.2 s",
            "(9/24, trial) Press Enter, then say:",
            "    Angel, what is on my calendar for this afternoon?",
            "Done: 24 clips in C:\\x. Delete that folder to erase them.",
        ]
        .iter()
        .filter_map(|l| r.line(l))
        .collect();
        assert_eq!(
            out,
            [
                Said::Prompt {
                    index: 3,
                    total: 24,
                    voiceprint: true,
                    sentence: "I would like a cup of coffee before the meeting starts at nine.".into()
                },
                Said::TooShort(0.4),
                Said::Prompt {
                    index: 3,
                    total: 24,
                    voiceprint: true,
                    sentence: "I would like a cup of coffee before the meeting starts at nine.".into()
                },
                Said::Saved(3.2),
                Said::Prompt {
                    index: 9,
                    total: 24,
                    voiceprint: false,
                    sentence: "Angel, what is on my calendar for this afternoon?".into()
                },
                Said::Done,
            ]
        );
    }

    #[test]
    fn a_run_is_read_by_its_registered_verdicts() {
        let v = json!({"genuine": vec![0.7; 16], "accepted_genuine": 16, "accepted_impostor": 0, "impostor_trials": 864,
                       "threshold": 0.5324, "separated": true, "verdicts": {"B1": true, "B2": true}});
        let run = Run::from_json(&v).unwrap();
        assert!(run.passed);
        assert!(run.words().contains("16 of 16") && run.words().contains("0 of 864") && run.words().contains("0.5324"));
        let mut failed = v.clone();
        failed["verdicts"]["B2"] = json!(false);
        assert!(!Run::from_json(&failed).unwrap().passed);
        assert!(Run::from_json(&json!({"threshold": 0.5})).is_none(), "not a run");
    }

    #[test]
    fn the_enrolment_is_read_from_its_files_without_the_vector() {
        let root = crate::sqlite_ro::tests::scratch("voice-read");
        let p = places(&root);
        assert!(!read(&p).enrolled(), "nothing there: not enrolled");
        let sp = p.speaker();
        fs::create_dir_all(sp.join("owner")).unwrap();
        fs::create_dir_all(p.scripts.join("results")).unwrap();
        fs::write(
            sp.join("decision.json"),
            r#"{"threshold": 0.5324, "model_sha256": "5ef2", "measured_by": "results\\run-1.json", "date": "2026-09-25"}"#,
        )
        .unwrap();
        fs::write(sp.join("voiceprint.json"), r#"{"vector": [0.1, 0.2], "utterances": 8, "created": "2026-09-25"}"#).unwrap();
        for i in 1..=24 {
            fs::write(sp.join("owner").join(format!("{i:02}.wav")), b"").unwrap();
        }
        fs::write(
            p.scripts.join("results").join("run-1.json"),
            r#"{"genuine": [0.7], "accepted_genuine": 1, "accepted_impostor": 0, "impostor_trials": 3, "threshold": 0.5, "verdicts": {"B1": true, "B2": true}}"#,
        )
        .unwrap();
        let e = read(&p);
        assert!(e.enrolled());
        assert_eq!((e.threshold, e.utterances, e.clips, e.date.as_str()), (Some(0.5324), Some(8), 24, "2026-09-25"));
        assert!(e.run.as_ref().is_some_and(|r| r.passed), "the run read beside the probe: {:?}", e.problems);
    }

    #[test]
    fn erasing_and_re_recording_move_things_aside_and_delete_nothing() {
        let root = crate::sqlite_ro::tests::scratch("voice-aside");
        let p = places(&root);
        let sp = p.speaker();
        fs::create_dir_all(sp.join("owner")).unwrap();
        fs::write(sp.join("owner").join("01.wav"), b"clip").unwrap();
        fs::write(sp.join("voiceprint.json"), b"{}").unwrap();
        fs::write(sp.join("decision.json"), b"{}").unwrap();
        // before a re-enrolment: only the clips move; the voiceprint stays in force
        let prev = set_aside(&sp, &["owner"], "previous", 1_791_400_000.0).unwrap().unwrap();
        assert!(prev.join("owner").join("01.wav").is_file() && !sp.join("owner").exists() && sp.join("voiceprint.json").is_file());
        assert_eq!(set_aside(&sp, &["owner"], "previous", 1_791_400_001.0).unwrap(), None, "nothing to move");
        // erase: the voiceprint and its decision go aside too
        fs::create_dir_all(sp.join("owner")).unwrap();
        let gone = erase(&p, 1_791_400_002.0).unwrap().unwrap();
        assert!(gone.join("voiceprint.json").is_file() && gone.join("decision.json").is_file() && gone.join("owner").is_dir());
        assert!(!read(&p).enrolled());
        assert_eq!(read(&p).aside.len(), 2, "both folders listed, nothing deleted");
        assert!(erase(&p, 1_791_400_003.0).unwrap().is_none(), "nothing left to erase");
    }

    #[test]
    fn an_erase_asks_twice_and_a_re_enrolment_needs_what_it_runs_on() {
        let root = crate::sqlite_ro::tests::scratch("voice-state");
        let p = places(&root);
        fs::create_dir_all(p.speaker().join("owner")).unwrap();
        fs::write(p.speaker().join("voiceprint.json"), b"{}").unwrap();
        let mut s = State { places: Ok(p.clone()), enrolment: None, phase: Phase::Idle };
        s.ask(true);
        s.confirm(1_791_400_000.0);
        assert!(matches!(s.phase, Phase::Confirm { erase: true, step: 2 }), "asked a second time");
        assert!(p.speaker().join("voiceprint.json").is_file(), "nothing moved yet");
        s.cancel();
        assert!(matches!(s.phase, Phase::Idle) && p.speaker().join("voiceprint.json").is_file(), "cancel moves nothing");
        s.ask(true);
        s.confirm(1_791_400_000.0);
        s.confirm(1_791_400_000.0);
        assert!(matches!(s.phase, Phase::Finished(Ok(_))) && !p.speaker().join("voiceprint.json").exists());
        // a re-enrolment with nothing to run on says what is missing, and moves nothing
        fs::create_dir_all(p.speaker().join("owner")).unwrap();
        s.phase = Phase::Idle;
        s.ask(false);
        s.confirm(1_791_400_100.0);
        assert!(matches!(&s.phase, Phase::Finished(Err(why)) if why.contains("runtime interpreter")));
        assert!(p.speaker().join("owner").is_dir(), "the clips stay where they are");
        assert!(!s.on_air() && !s.running());
    }

    /// By hand (`cargo test -- --ignored the_whole_re_enrolment`, with Python on PATH): the window drives stand-ins for
    /// the two scripts that speak as they do (no microphone, nobody's voice), through every sentence to a passing result.
    #[test]
    #[ignore]
    fn the_whole_re_enrolment_runs_through_stand_ins() {
        let root = crate::sqlite_ro::tests::scratch("voice-e2e");
        let mut p = places(&root);
        p.python = PathBuf::from(
            String::from_utf8(Command::new("where").arg("python").output().unwrap().stdout).unwrap().lines().next().unwrap().trim(),
        );
        fs::create_dir_all(&p.scripts).unwrap();
        fs::create_dir_all(&p.models).unwrap();
        for f in ["kokoro-v1.0.onnx", "voices-v1.0.bin", MODEL_FILE] {
            fs::write(p.models.join(f), b"").unwrap();
        }
        fs::write(
            p.scripts.join("record_owner.py"),
            "import sys\nprint('Speaker probe, part B')\nfor i in range(24):\n    print(f'({i+1}/24, {\"voiceprint\" if i < 8 else \"trial\"}) Press Enter, then say:')\n    print('')\n    print(f'    Sentence number {i+1}.')\n    sys.stdin.readline()\n    print('  saved 2.0 s')\nprint('Done: 24 clips')\n",
        )
        .unwrap();
        fs::write(
            p.scripts.join("threshold_probe.py"),
            "import sys, json\nout = sys.argv[sys.argv.index('--out') + 1]\nimport os\nos.makedirs(os.path.dirname(out), exist_ok=True)\njson.dump({'genuine': [0.7]*16, 'accepted_genuine': 16, 'accepted_impostor': 0, 'impostor_trials': 864, 'threshold': 0.53, 'separated': True, 'verdicts': {'B1': True, 'B2': True}}, open(out, 'w'))\n",
        )
        .unwrap();
        fs::create_dir_all(p.speaker().join("owner")).unwrap();
        fs::write(p.speaker().join("owner").join("01.wav"), b"old clip").unwrap();
        let mut s = State { places: Ok(p.clone()), enrolment: None, phase: Phase::Idle };
        s.ask(false);
        s.confirm(1_791_400_000.0);
        assert!(s.running(), "{:?}", matches!(s.phase, Phase::Finished(_)));
        assert!(read(&p).aside.iter().any(|a| a.starts_with("previous-")), "the old clips moved aside first");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut recorded = 0;
        while s.running() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
            s.tick(1_791_400_100.0);
            if let Phase::Recording { prompt: Some(_), listening: false, .. } = &s.phase {
                s.record_next();
                assert!(s.on_air(), "the microphone is on air while a sentence records");
                recorded += 1;
            }
        }
        match &s.phase {
            Phase::Finished(Ok(words)) => assert!(words.contains("16 of 16") && words.contains("Enrolled"), "{words}"),
            Phase::Finished(Err(why)) => panic!("{why}"),
            _ => panic!("did not finish"),
        }
        assert_eq!(recorded, 24, "one press a sentence");
        assert!(!s.on_air());
    }

    /// By hand (`cargo test -- --ignored a_real_run`, with `CENTCOM_VOICE_RUN` naming a results file of the real
    /// threshold_probe.py): it reads with the same verdict the script printed.
    #[test]
    #[ignore]
    fn a_real_run_is_read() {
        let path = PathBuf::from(std::env::var("CENTCOM_VOICE_RUN").expect("CENTCOM_VOICE_RUN"));
        let run = Run::from_json(&read_json(&path).unwrap()).expect("a run");
        println!("{}", run.words());
    }

    #[test]
    fn what_a_re_enrolment_needs_is_named() {
        let root = crate::sqlite_ro::tests::scratch("voice-missing");
        let m = places(&root).missing();
        assert_eq!(m.len(), 6, "{m:?}");
        assert!(m[0].contains("runtime interpreter"));
    }
}

// ------------------------------------------------------------------- the window's side

/// Where a re-enrolment or an erase stands.
pub enum Phase {
    Idle,
    /// Asking before it acts: a re-enrolment once, an erase twice (`step` is the question on show, 1 or 2).
    Confirm {
        erase: bool,
        step: u8,
    },
    /// Recording: the sentence on show, whether the microphone is open for it now, and how many clips are kept.
    Recording {
        script: Script,
        reader: Reader,
        prompt: Option<Said>,
        listening: bool,
        saved: usize,
        note: Option<String>,
        tail: Vec<String>,
    },
    /// The registered scoring, writing its results to `out`.
    Scoring {
        script: Script,
        out: PathBuf,
        tail: Vec<String>,
    },
    /// How the last attempt ended, in words: Ok when it passed, Err otherwise.
    Finished(Result<String, String>),
}

pub struct State {
    pub places: Result<Places, String>,
    pub enrolment: Option<Enrolment>,
    pub phase: Phase,
}

impl Default for State {
    fn default() -> State {
        State { places: Places::find(), enrolment: None, phase: Phase::Idle }
    }
}

/// The questions a confirmation asks.
pub fn question(erase: bool, step: u8) -> &'static str {
    match (erase, step) {
        (false, _) => {
            "Record the 24 sentences again? Your clips now are moved aside first and kept (speaker/previous-<time>). \
                       Then the registered test runs on the processor for about ten minutes; the voiceprint changes only if it \
                       passes, and Sinai's loop uses it from its next start."
        }
        (true, 1) => {
            "Erase the voiceprint? It, its decision and your clips are moved aside (speaker/erased-<time>), not deleted. \
                      From Sinai's next start, any voice can give it spoken commands that grant authority, until you enrol again."
        }
        (true, _) => "Are you sure? Any voice will be able to grant Sinai authority by speaking once its loop restarts.",
    }
}

impl State {
    /// Read the enrolment again (two small files and a folder).
    pub fn refresh(&mut self) {
        if let Ok(p) = &self.places {
            self.enrolment = Some(read(p));
        }
    }

    /// The folder the enrolment lives in (`speaker`), which the Sinai page watches; None when it has no place.
    pub fn speaker_folder(&self) -> Option<std::path::PathBuf> {
        self.places.as_ref().ok().map(Places::speaker)
    }

    /// Busy: a script runs.
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Scoring { .. })
    }

    /// The microphone is open for a sentence now: the on-air bar says so.
    pub fn on_air(&self) -> bool {
        matches!(self.phase, Phase::Recording { listening: true, .. })
    }

    pub fn ask(&mut self, erase: bool) {
        if !self.running() {
            self.phase = Phase::Confirm { erase, step: 1 };
        }
    }

    pub fn cancel(&mut self) {
        if let Phase::Confirm { .. } | Phase::Finished(_) = self.phase {
            self.phase = Phase::Idle;
        }
    }

    /// A confirmation pressed: an erase asks twice; then it acts.
    pub fn confirm(&mut self, now: f64) {
        let Phase::Confirm { erase, step } = self.phase else { return };
        let Ok(places) = self.places.clone() else { return };
        if erase && step < 2 {
            self.phase = Phase::Confirm { erase, step: step + 1 };
            return;
        }
        self.phase = if erase {
            match self::erase(&places, now) {
                Ok(Some(dest)) => Phase::Finished(Ok(format!(
                    "Erased: moved aside into {}. Sinai's loop stops gating by voice from its next start.",
                    dest.display()
                ))),
                Ok(None) => Phase::Finished(Err("There was nothing to erase.".into())),
                Err(why) => Phase::Finished(Err(why)),
            }
        } else {
            let missing = places.missing();
            if !missing.is_empty() {
                Phase::Finished(Err(format!("Re-enrolling needs: {}.", missing.join("; "))))
            } else {
                match set_aside(&places.speaker(), &["owner"], "previous", now).and_then(|_| Script::record(&places)) {
                    Ok(script) => Phase::Recording {
                        script,
                        reader: Reader::default(),
                        prompt: None,
                        listening: false,
                        saved: 0,
                        note: None,
                        tail: Vec::new(),
                    },
                    Err(why) => Phase::Finished(Err(why)),
                }
            }
        };
        self.refresh();
    }

    /// Record the sentence on show: the recorder opens the microphone until the sentence ends.
    pub fn record_next(&mut self) {
        if let Phase::Recording { script, prompt: Some(_), listening, note, .. } = &mut self.phase {
            if !*listening && script.enter() {
                *listening = true;
                *note = None;
            }
        }
    }

    /// Stop whatever runs; what was recorded so far stays, and the voiceprint before stays in force.
    pub fn stop(&mut self) {
        if self.running() {
            if let Phase::Recording { script, .. } | Phase::Scoring { script, .. } = &mut self.phase {
                script.stop();
            }
            self.phase = Phase::Finished(Err(
                "Stopped. The voiceprint before this stays in force; the clips recorded so far are in speaker/owner.".into(),
            ));
            self.refresh();
        }
    }

    /// Read what the running script said, and move on when it ends.
    pub fn tick(&mut self, now: f64) {
        let mut next: Option<Phase> = None;
        match &mut self.phase {
            Phase::Recording { script, reader, prompt, listening, saved, note, tail } => {
                while let Ok(line) = script.lines.try_recv() {
                    keep_tail(tail, &line);
                    match reader.line(&line) {
                        Some(p @ Said::Prompt { .. }) => *prompt = Some(p),
                        Some(Said::Saved(_)) => {
                            *saved += 1;
                            *listening = false;
                        }
                        Some(Said::TooShort(s)) => {
                            *listening = false;
                            *note = Some(format!("Heard only {s:.1} s; please say it again."));
                        }
                        Some(Said::Done) | None => {}
                    }
                }
                if let Some(code) = script.ended() {
                    let places = self.places.clone();
                    next = Some(match (code, places) {
                        (Some(0), Ok(places)) if *saved >= SENTENCES => {
                            let out = places.speaker().join("runs").join(format!("run-{}.json", stamp(now)));
                            match Script::score(&places, &out) {
                                Ok(script) => Phase::Scoring { script, out, tail: Vec::new() },
                                Err(why) => Phase::Finished(Err(why)),
                            }
                        }
                        _ => Phase::Finished(Err(format!("The recorder ended before all {SENTENCES} clips: {}", tail.join(" / ")))),
                    });
                }
            }
            Phase::Scoring { script, out, tail } => {
                while let Ok(line) = script.lines.try_recv() {
                    keep_tail(tail, &line);
                }
                if script.ended().is_some() {
                    next = Some(match read_json(out).ok().and_then(|v| Run::from_json(&v)) {
                        Some(run) if run.passed => {
                            Phase::Finished(Ok(format!("Enrolled: {}. Sinai's loop uses it from its next start.", run.words())))
                        }
                        Some(run) => Phase::Finished(Err(format!("{}. The voiceprint before this stays in force.", run.words()))),
                        None => Phase::Finished(Err(format!("The test wrote no result: {}", tail.join(" / ")))),
                    });
                }
            }
            _ => {}
        }
        if let Some(phase) = next {
            self.phase = phase;
            self.refresh();
        }
    }
}

/// The last few lines a script wrote, for when it fails.
fn keep_tail(tail: &mut Vec<String>, line: &str) {
    let line = line.trim();
    if !line.is_empty() {
        tail.push(crate::ui::cut(line, 200).to_string());
        if tail.len() > 4 {
            tail.remove(0);
        }
    }
}
