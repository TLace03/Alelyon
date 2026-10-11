//! The part picker's rules: what each part offers, how a choice is kept and read back (beside the models panel's
//! `sinai_model`), what the speech engine is started with, and what each card says. Every test works in a scratch
//! folder: none reads or writes the person's preferences or models.

use super::*;
use crate::models::{Endpoint, Gguf};

fn scratch(name: &str) -> PathBuf {
    crate::sqlite_ro::tests::scratch(name)
}

#[test]
fn each_kind_is_kept_as_its_entry_and_read_back() {
    for c in [
        Choice::SinaiMind,
        Choice::Gguf("qwen3-8b".into()),
        Choice::Endpoint("lm-studio".into()),
        Choice::SpeechFile("ggml-base.en.bin".into()),
        Choice::Cloud { provider: "openrouter".into(), model: "meta/llama:free".into(), agreed: None },
        Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: Some("2026-10-10T12:00:00Z".into()) },
    ] {
        assert_eq!(Choice::from_entry(&c.entry()), Some(c.clone()), "{c:?}");
    }
    let unknown = json!({"kind": "telepathy", "range": 3});
    assert_eq!(Choice::from_entry(&unknown), Some(Choice::Unknown(unknown.clone())), "kept as written");
    assert_eq!(Choice::Unknown(unknown.clone()).entry(), unknown);
    assert_eq!(Choice::from_entry(&json!({"kind": "gguf"})), None, "a model kind without its model is no choice");
    assert_eq!(Choice::from_entry(&json!({"name": "x"})), None, "no kind");
    for path in ["../ggml.bin", "C:/models/ggml.bin", "sub\\ggml.bin", ".."] {
        assert_eq!(Choice::from_entry(&json!({"kind": "speech-file", "file": path})), None, "{path}: a file name, never a path");
    }
}

#[test]
fn each_part_offers_its_kinds_with_a_free_cloud_model_last() {
    assert_eq!(offered(Part::Mind, true), [Kind::SinaiMind, Kind::Gguf, Kind::Endpoint, Kind::Cloud], "Sinai's mind first, where it is installed");
    assert_eq!(offered(Part::Mind, false), [Kind::Gguf, Kind::Endpoint, Kind::Cloud], "no Sinai's mind in a build without it");
    assert_eq!(offered(Part::Ears, true), [Kind::SpeechFile, Kind::Cloud]);
    assert_eq!(offered(Part::Voice, false), [Kind::Endpoint, Kind::Cloud]);
    for part in Part::ALL {
        for mind in [false, true] {
            assert_eq!(offered(part, mind).last(), Some(&Kind::Cloud), "{part:?}: a model on this PC comes first");
            if part != Part::Mind {
                assert!(!offered(part, mind).contains(&Kind::SinaiMind), "{part:?}");
            }
        }
    }
}

#[test]
fn each_parts_free_cloud_default_is_the_approved_one_and_gemini_is_never_a_default() {
    let default = |p: Part| {
        let o = cloud::offers(p)[0];
        (o.cloud.id, o.model)
    };
    assert_eq!(default(Part::Mind), ("openrouter", None), "the mind picks from OpenRouter's live free list");
    assert_eq!(default(Part::Eyes), ("cloudflare", Some("@cf/meta/llama-3.2-11b-vision-instruct")));
    assert_eq!(default(Part::Ears), ("groq", Some("whisper-large-v3-turbo")));
    assert_eq!(default(Part::Embeddings), ("cloudflare", Some("@cf/baai/bge-m3")));
    assert_eq!(default(Part::Voice), ("cloudflare", Some("@cf/myshell-ai/melotts")));
    for part in Part::ALL {
        let offers = cloud::offers(part);
        assert!(offers[0].caveat.is_none(), "{part:?}: the default carries no caveat");
        for o in &offers {
            assert_eq!(o.cloud.trains, o.caveat.is_some(), "{part:?} {}: a provider that trains on what it is sent is labelled", o.cloud.id);
        }
        assert!(!cloud::sends(part).is_empty());
    }
    assert!(cloud::offers(Part::Ears).iter().all(|o| o.cloud.id != "gemini"), "no Gemini speech to text here");
    assert!(!cloud::allowed(Part::Mind, "openrouter", "openai/gpt-4o"), "a paid OpenRouter model is never allowed");
    assert!(cloud::allowed(Part::Mind, "openrouter", "qwen/qwen3-8b:free"));
    assert!(!cloud::allowed(Part::Ears, "openrouter", "qwen/qwen3-8b:free"), "a provider the part does not offer");
}

#[test]
fn a_cloud_choice_is_used_only_once_agreed_and_the_eyes_stay_off_after_other_agreements() {
    let agreed = |provider: &str, model: &str| Choice::Cloud { provider: provider.into(), model: model.into(), agreed: Some("2026-10-10T12:00:00Z".into()) };
    let mut c = Choices::default();
    c.set(Part::Ears, Some(Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: None }));
    assert!(c.get(Part::Ears).is_some() && c.used(Part::Ears).is_none(), "not agreed: kept, never used");
    assert!(ears_model_args(&c, None).is_empty(), "nothing goes to the speech engine before Agree");
    c.set(Part::Ears, Some(agreed("groq", "whisper-large-v3-turbo")));
    c.set(Part::Mind, Some(agreed("openrouter", "qwen/qwen3-8b:free")));
    c.set(Part::Voice, Some(agreed("cloudflare", "@cf/myshell-ai/melotts")));
    assert!(c.used(Part::Ears).is_some() && c.used(Part::Mind).is_some() && c.used(Part::Voice).is_some());
    assert_eq!(c.used(Part::Eyes), None, "agreeing for other parts never turns the eyes on");
    assert_eq!(
        ears_model_args(&c, Some(Path::new("M"))),
        ["--cloud", "groq", "--cloud-model", "whisper-large-v3-turbo"].map(String::from),
        "the speech engine reads in the cloud"
    );
    let given = alelyon_ears::setup::Given::default().with_args(&ears_model_args(&c, None));
    assert!(given.missing(|_| false).is_empty(), "nothing on this PC is needed for the ears in the cloud");
    // a paid model, or a provider the part does not offer, is never used even with a stamp
    c.set(Part::Mind, Some(agreed("openrouter", "openai/gpt-4o")));
    assert_eq!(c.used(Part::Mind), None);
    c.set(Part::Ears, Some(agreed("cloudflare", "@cf/openai/whisper")));
    assert_eq!(c.used(Part::Ears), None);
    assert_eq!(crate::sinai_parts::cloud::CloudPick::of(Some(&agreed("openrouter", "qwen/qwen3-8b:free"))).map(|p| p.label()), Some("qwen/qwen3-8b:free (OpenRouter)".to_string()));
    assert!(cloud::ears_env(None).unwrap().is_none());
    assert!(cloud::ears_env(Some(&agreed("groq", "whisper-large-v3-turbo"))).is_err(), "no key found (a test build finds none): no start");
}

#[test]
fn a_cloud_choice_without_agreement_is_refused_and_nothing_is_kept() {
    let dir = scratch("parts-cloud-refused");
    let at = dir.join("preferences.json");
    let mut s = State::at(Some(at.clone()), None);
    let unagreed = Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: None };
    assert!(matches!(s.update(Msg::Choose(Part::Ears, Some(unagreed)), None, false), Then::Nothing));
    assert!(matches!(&s.said, Some(Err(w)) if w.contains("nothing is sent before you agree")), "{:?}", s.said);
    assert!(!at.exists(), "nothing was written");
    let wrong = Choice::Cloud { provider: "openrouter".into(), model: "openai/gpt-4o".into(), agreed: Some("2026-10-10T12:00:00Z".into()) };
    assert!(matches!(s.update(Msg::Choose(Part::Mind, Some(wrong)), None, false), Then::Nothing));
    assert!(matches!(&s.said, Some(Err(w)) if w.contains("no free model")), "{:?}", s.said);
    assert!(!at.exists());
}

#[test]
fn the_steps_connect_choose_and_agree_and_only_agree_keeps_the_choice() {
    use cloud::{Out, Stage, Step};
    let mut flow = None;
    // a test build finds no key: the steps start at Connect, and nothing is sent
    assert!(matches!(cloud::update(&mut flow, Step::Start(Part::Ears, "groq")), Out::Nothing));
    let f = flow.as_ref().unwrap();
    assert_eq!((f.stage, f.model.as_deref()), (Stage::Connect, Some("whisper-large-v3-turbo")));
    // Agree before choosing does nothing
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Nothing));
    assert!(flow.is_some());
    // the live list read: the suggested model is still there
    let listed = vec![lattice_core::hosted::HostedModel { id: "whisper-large-v3-turbo".into(), name: "whisper-large-v3-turbo".into(), context: None, price: None }];
    let _ = cloud::update(&mut flow, Step::Listed(Ok(listed)));
    assert_eq!(flow.as_ref().unwrap().model.as_deref(), Some("whisper-large-v3-turbo"));
    let _ = cloud::update(&mut flow, Step::Pick("whisper-large-v3-turbo".into()));
    assert_eq!(flow.as_ref().unwrap().stage, Stage::Agree);
    match cloud::update(&mut flow, Step::Agree) {
        Out::Choose(Part::Ears, Choice::Cloud { provider, model, agreed: Some(at) }) => {
            assert_eq!((provider.as_str(), model.as_str()), ("groq", "whisper-large-v3-turbo"));
            assert!(at.starts_with("20") && at.ends_with('Z') && at.contains('T'), "{at}");
        }
        _ => panic!("Agree keeps the choice"),
    }
    assert!(flow.is_none(), "the steps close once agreed");
    // a model that is not free cannot be picked; Not now closes without keeping anything
    let _ = cloud::update(&mut flow, Step::Start(Part::Mind, "openrouter"));
    let _ = cloud::update(&mut flow, Step::Pick("openai/gpt-4o".into()));
    let f = flow.as_ref().unwrap();
    assert_eq!(f.stage, Stage::Connect);
    assert!(f.said.as_deref().is_some_and(|w| w.contains("free models only")));
    assert!(matches!(cloud::update(&mut flow, Step::Close), Out::Nothing));
    assert!(flow.is_none());
    // through the picker: an agreed choice goes the way every choice is kept
    let mut s = State::at(None, None);
    s.open = Some(Part::Voice);
    let _ = s.update(Msg::Cloud(Step::Start(Part::Voice, "cloudflare")), None, false);
    let _ = s.update(Msg::Cloud(Step::Pick("@cf/myshell-ai/melotts".into())), None, false);
    let _ = s.update(Msg::Cloud(Step::Agree), None, false);
    assert!(matches!(&s.said, Some(Err(w)) if w.contains("keeps no preferences")), "Agree reached the keeping: {:?}", s.said);
}

#[test]
fn a_part_at_its_free_limit_pauses_and_its_card_says_when_it_is_back() {
    let knocked = HashMap::new();
    let down = |_: u16| Some(false);
    let mut c = Choices::default();
    c.set(Part::Embeddings, Some(Choice::Cloud { provider: "cloudflare".into(), model: "@cf/baai/bge-m3".into(), agreed: Some("2026-10-10T12:00:00Z".into()) }));
    let card_now = card(Part::Embeddings, &c, &facts(false, &Answering::Nobody, None, &knocked, &down));
    assert_eq!((card_now.model.as_str(), card_now.runs.as_str()), ("@cf/baai/bge-m3", "in the cloud (Cloudflare Workers AI)"));
    assert!(card_now.used_by.starts_with("Nothing in this build uses it yet"), "{}", card_now.used_by);
    let until = cloud::now() + 3600;
    cloud::pause(Part::Embeddings, "cloudflare", until);
    let paused = card(Part::Embeddings, &c, &facts(false, &Answering::Nobody, None, &knocked, &down));
    assert_eq!(paused.level, Level::Problem);
    assert!(paused.state.starts_with("Free limit reached, back at "), "{}", paused.state);
    assert_eq!(cloud::paused(Part::Embeddings, until), None, "the pause ends at its time");
    assert!(cloud::back_words(cloud::now() + 2 * 86_400, cloud::now()).contains(" on "), "another day names its day");
    // the ears in the cloud: not connected (no key found) says so; connected, it follows the speech engine
    let mut c = Choices::default();
    c.set(Part::Ears, Some(Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: Some("2026-10-10T12:00:00Z".into()) }));
    cloud::test_keys(&[]);
    let missing = card(Part::Ears, &c, &facts(false, &Answering::Nobody, None, &knocked, &down));
    assert_eq!(missing.level, Level::Problem);
    assert!(missing.state.starts_with("Not connected"), "{}", missing.state);
    cloud::test_keys(&["GROQ_API_KEY"]);
    let mut c = Choices::default();
    c.set(Part::Ears, Some(Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: Some("2026-10-10T12:00:00Z".into()) }));
    let mut f = facts(false, &Answering::Nobody, None, &knocked, &down);
    f.ears = EarsNow::Running { recognizer: "ready".into(), model: "ggml-large-v3-turbo-q5_0.bin".into() };
    let ears = card(Part::Ears, &c, &f);
    assert_eq!(ears.runs, "in the cloud (Groq)");
    assert!(ears.state.contains("next starts"), "{}", ears.state);
    f.ears = EarsNow::Running { recognizer: "ready".into(), model: "whisper-large-v3-turbo (Groq, in the cloud)".into() };
    assert!(card(Part::Ears, &c, &f).state.starts_with("Sends your voice"), "{}", card(Part::Ears, &c, &f).state);
    cloud::test_keys(&[]);
}

#[test]
fn cloudflares_parts_share_one_metered_count_and_pause_before_the_free_allocation() {
    use lattice_core::hosted::meter::{self, Meter};
    let dir = scratch("parts-cloud-meter");
    let path = meter::path_in(&dir);
    cloud::test_meter(Some(path.clone()));
    cloud::test_keys(&["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"]);
    let account = "stub-key-for-tests"; // what a test thread's lookup finds for the account id
    let knocked = HashMap::new();
    let down = |_: u16| Some(false);
    let agreed = |model: &str| Choice::Cloud { provider: "cloudflare".into(), model: model.into(), agreed: Some("2026-10-10T12:00:00Z".into()) };
    let mut c = Choices::default();
    c.set(Part::Embeddings, Some(agreed("@cf/baai/bge-m3")));
    c.set(Part::Voice, Some(agreed("@cf/myshell-ai/melotts")));
    let now = cloud::now();
    Meter::at(path.clone()).add(account, 4_000.0, now).unwrap(); // the embeddings' calls today
    let voice = card(Part::Voice, &c, &facts(false, &Answering::Nobody, None, &knocked, &down));
    assert_eq!(voice.level, Level::Up);
    assert!(voice.state.contains("Today 4000 of the 9000 Neurons"), "the voice reads the account's one count: {}", voice.state);
    Meter::at(path.clone()).add(account, 4_999.5, now).unwrap(); // the voice's
    for part in [Part::Embeddings, Part::Voice] {
        let paused = card(part, &c, &facts(false, &Answering::Nobody, None, &knocked, &down));
        assert_eq!(paused.level, Level::Problem, "{part:?}");
        assert!(paused.state.starts_with("Free limit reached, back at "), "{part:?}: {}", paused.state);
    }
    assert_eq!(cloud::cloudflare_spent(now), Some(meter::next_midnight(now)), "back at 00:00 UTC");
    assert!(cloud::cloudflare_spent(meter::next_midnight(now)).is_none(), "a new day's count starts at nothing");
    // a Cloudflare model without a published rate the meter knows is never allowed
    assert!(!cloud::allowed(Part::Embeddings, "cloudflare", "@cf/someone/new-embedder"));
    assert!(cloud::allowed(Part::Embeddings, "cloudflare", "@cf/qwen/qwen3-embedding-0.6b"));
    cloud::test_meter(None);
    cloud::test_keys(&[]);
}

#[test]
fn the_eyes_agree_waits_for_the_licence_tick_and_sends_the_acceptance_once() {
    use cloud::{Out, Stage, Step};
    let model = "@cf/meta/llama-3.2-11b-vision-instruct";
    cloud::test_keys(&["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"]);
    let at_agree = || {
        let mut flow = None;
        let _ = cloud::update(&mut flow, Step::Start(Part::Eyes, "cloudflare"));
        // connected (the test's made-up keys): the live list is read, here as Cloudflare would list it
        let listed = vec![lattice_core::hosted::HostedModel { id: model.into(), name: model.into(), context: None, price: None }];
        let _ = cloud::update(&mut flow, Step::Listed(Ok(listed)));
        let _ = cloud::update(&mut flow, Step::Pick(model.into()));
        assert_eq!(flow.as_ref().unwrap().stage, Stage::Agree);
        flow
    };
    // not ticked: Agree keeps nothing and sends nothing
    let mut flow = at_agree();
    assert!(!flow.as_ref().unwrap().licence_ticked, "the tick starts unticked");
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Nothing));
    assert!(flow.as_ref().unwrap().said.as_deref().is_some_and(|w| w.contains("I accept the Llama 3.2 licence")));
    // ticked, not yet accepted on this account: the acceptance is sent first, then the choice is kept
    let _ = cloud::update(&mut flow, Step::Licence(true));
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Task(_)), "the acceptance goes on the account");
    assert!(flow.as_ref().unwrap().busy);
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Nothing), "not twice while it goes");
    let _ = cloud::update(&mut flow, Step::LicenceSent(Err("Cloudflare could not be reached.".into())));
    assert!(flow.is_some(), "a refused acceptance keeps nothing");
    assert!(matches!(cloud::update(&mut flow, Step::LicenceSent(Ok(()))), Out::Choose(Part::Eyes, Choice::Cloud { .. })));
    assert!(flow.is_none());
    // accepted once on this account and remembered: a later Agree (still ticked) sends nothing more
    cloud::remember_licence("stub-key-for-tests", model, cloud::now()).unwrap();
    assert!(cloud::licence_accepted("stub-key-for-tests", model));
    let mut flow = at_agree();
    let _ = cloud::update(&mut flow, Step::Licence(true));
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Choose(Part::Eyes, _)), "no second acceptance");
    // another part has no licence to tick
    let mut flow = None;
    let _ = cloud::update(&mut flow, Step::Start(Part::Voice, "cloudflare"));
    let _ = cloud::update(&mut flow, Step::Listed(Ok(Vec::new())));
    let _ = cloud::update(&mut flow, Step::Pick("@cf/myshell-ai/melotts".into()));
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Choose(Part::Voice, _)));
    cloud::test_keys(&[]);
}

#[test]
fn a_pause_is_kept_on_disk_until_its_time_and_the_ears_start_paused() {
    let dir = scratch("parts-cloud-pauses");
    let path = dir.join("cloud-pauses.json");
    let now = 1_791_633_600;
    let mut pauses = HashMap::new();
    pauses.insert(Part::Ears, cloud::Paused { provider: "groq".into(), until: now + 600 });
    pauses.insert(Part::Mind, cloud::Paused { provider: "openrouter".into(), until: now - 1 });
    cloud::save_pauses(&path, &pauses).unwrap();
    // a restart reads the file: the ears' pause still lasts, the mind's has passed and is dropped
    let read = cloud::load_pauses(&path, now);
    assert_eq!(read.get(&Part::Ears), Some(&cloud::Paused { provider: "groq".into(), until: now + 600 }));
    assert!(!read.contains_key(&Part::Mind));
    assert!(cloud::load_pauses(&path, now + 600).is_empty(), "nothing lasts past its time");
    assert!(cloud::load_pauses(&dir.join("none.json"), now).is_empty());
    // the speech engine starts paused for a limit already known
    let ears = Choice::Cloud { provider: "groq".into(), model: "whisper-large-v3-turbo".into(), agreed: Some("2026-10-10T12:00:00Z".into()) };
    let args = cloud::ears_args_with(Some(&ears), Some(now + 600));
    assert_eq!(&args[4..], ["--cloud-paused-until".to_string(), (now + 600).to_string()]);
    assert_eq!(cloud::ears_args_with(Some(&ears), None).len(), 4);
}

#[test]
fn the_minds_cloud_model_is_kept_beside_sinai_model_and_read_back() {
    let dir = scratch("parts-cloud-mind");
    let at = dir.join("preferences.json");
    save(&at, Part::Mind, Some(&Choice::Gguf("qwen".into()))).unwrap();
    let pick = Choice::Cloud { provider: "openrouter".into(), model: "qwen/qwen3-8b:free".into(), agreed: Some("2026-10-10T12:00:00Z".into()) };
    save(&at, Part::Mind, Some(&pick)).unwrap();
    assert_eq!(crate::prefs::load_sinai_model(&at).as_deref(), Some("cloud:openrouter/qwen/qwen3-8b:free"));
    assert_eq!(crate::models::Pick::parse("cloud:openrouter/qwen/qwen3-8b:free"), None, "older versions read it as no model");
    assert_eq!(Choices::read(Some(&at)).used(Part::Mind), Some(&pick), "the cloud model is the mind's, not the older own model");
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&at).unwrap()).unwrap();
    assert_eq!(value["parts"]["mind"]["agreed"], "2026-10-10T12:00:00Z");
    assert!(!value.to_string().contains("KEY"), "never a key in the preferences");
}

#[test]
fn the_mind_reads_sinai_model_beside_its_own_entry() {
    let parts = |v: Value| v.as_object().cloned().unwrap();
    // an older version kept only sinai_model: the mind's model is that one
    let c = Choices::from_prefs(&parts(json!({})), Some("gguf:qwen"));
    assert_eq!(c.get(Part::Mind), Some(&Choice::Gguf("qwen".into())));
    // an older version chose after this one: sinai_model wins
    let c = Choices::from_prefs(&parts(json!({"mind": {"kind": "gguf", "name": "old"}})), Some("endpoint:lm"));
    assert_eq!(c.get(Part::Mind), Some(&Choice::Endpoint("lm".into())));
    // Sinai's mind chosen: the own model kept beside it does not replace it
    let c = Choices::from_prefs(&parts(json!({"mind": {"kind": "sinai-mind"}})), Some("gguf:qwen"));
    assert_eq!(c.get(Part::Mind), Some(&Choice::SinaiMind));
    // a cloud or unknown entry is kept and not used: the part uses its default
    let c = Choices::from_prefs(&parts(json!({"eyes": {"kind": "cloud", "provider": "p", "model": "m"}, "voice": {"kind": "new"}})), None);
    assert!(c.get(Part::Eyes).is_some() && c.used(Part::Eyes).is_none());
    assert!(c.get(Part::Voice).is_some() && c.used(Part::Voice).is_none());
    assert_eq!(Choices::read(None), Choices::default(), "no preferences, no choices");
}

#[test]
fn a_choice_is_kept_per_part_and_the_minds_also_as_sinai_model() {
    let dir = scratch("parts-save");
    let at = dir.join("preferences.json");
    save(&at, Part::Eyes, Some(&Choice::Gguf("vision".into()))).unwrap();
    save(&at, Part::Mind, Some(&Choice::Endpoint("lm".into()))).unwrap();
    save(&at, Part::Ears, Some(&Choice::SpeechFile("ggml-base.en.bin".into()))).unwrap();
    assert_eq!(crate::prefs::load_sinai_model(&at).as_deref(), Some("endpoint:lm"), "the models panel reads the mind's model");
    let c = Choices::read(Some(&at));
    assert_eq!(c.get(Part::Eyes), Some(&Choice::Gguf("vision".into())));
    assert_eq!(c.get(Part::Mind), Some(&Choice::Endpoint("lm".into())));
    // Sinai's mind chosen: sinai_model, the models panel's own model, is left as it was
    save(&at, Part::Mind, Some(&Choice::SinaiMind)).unwrap();
    assert_eq!(crate::prefs::load_sinai_model(&at).as_deref(), Some("endpoint:lm"));
    assert_eq!(Choices::read(Some(&at)).get(Part::Mind), Some(&Choice::SinaiMind));
    // the default: the entry is forgotten
    save(&at, Part::Eyes, None).unwrap();
    assert_eq!(Choices::read(Some(&at)).get(Part::Eyes), None);
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&at).unwrap()).unwrap();
    assert_eq!(value["parts"]["version"], crate::prefs::PARTS_VERSION);
    // one action can fill several parts: the store holds a choice per part
    let mut c = Choices::default();
    for part in [Part::Mind, Part::Eyes, Part::Embeddings] {
        c.set(part, Some(Choice::Gguf("multimodal".into())));
    }
    assert_eq!(Part::ALL.iter().filter(|p| c.get(**p).is_some()).count(), 3);
}

#[test]
fn the_speech_engine_starts_with_the_chosen_file_from_its_own_folder() {
    let dir = PathBuf::from("M");
    let mut c = Choices::default();
    assert!(ears_model_args(&c, Some(&dir)).is_empty(), "the default: the engine's own rule");
    c.set(Part::Ears, Some(Choice::SpeechFile("ggml-base.en.bin".into())));
    let args = ears_model_args(&c, Some(&dir));
    assert_eq!(args, ["--model".to_string(), dir.join("ggml-base.en.bin").display().to_string()]);
    assert!(ears_model_args(&c, None).is_empty(), "no folder known, nothing named");
    // the engine reads the last --model it is given: options typed for the engine afterwards still win
    let given = alelyon_ears::setup::Given::default().with_args(&[args[0].as_str(), args[1].as_str(), "--model", "typed.bin"]);
    assert_eq!(given.model_path(), Some(PathBuf::from("typed.bin")));
    let ours = alelyon_ears::setup::Given::default().with_args(&args);
    assert_eq!(ours.model_path(), Some(dir.join("ggml-base.en.bin")));
    assert!(chosen_ears_args().is_empty(), "a test build keeps no preferences");
}

#[test]
fn only_whisper_model_files_in_the_folder_itself_are_listed() {
    let dir = scratch("parts-speech");
    std::fs::write(dir.join("ggml-base.en.bin"), b"lmgg\x00\x00\x00\x01").unwrap();
    std::fs::write(dir.join("voices.bin"), b"\x93NUMPY").unwrap();
    std::fs::write(dir.join("notes.txt"), b"lmgg").unwrap();
    std::fs::write(dir.join("tiny.bin"), b"lm").unwrap();
    std::fs::create_dir_all(dir.join("below")).unwrap();
    std::fs::write(dir.join("below").join("ggml-tiny.bin"), b"lmgg").unwrap();
    let found = speech_files(&dir);
    assert_eq!(found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["ggml-base.en.bin"]);
    assert_eq!(found[0].size, 8);
    assert!(speech_files(&dir.join("nowhere")).is_empty());
}

fn catalog(dir: &Path) -> Catalog {
    Catalog {
        models_dir: dir.to_path_buf(),
        server: Ok(dir.join("llama-server.exe")),
        ggufs: vec![Gguf { name: "qwen".into(), path: dir.join("qwen.gguf"), size: 10 }],
        endpoints: vec![Endpoint {
            id: "lm".into(),
            label: "LM Studio".into(),
            address: "http://127.0.0.1:1234/v1".into(),
            model: "m".into(),
            local: true,
            configured: true,
            status: "Set up.".into(),
            key_kept: None,
            needs_key: false,
            knock: Some(("127.0.0.1".into(), 1234)),
        }],
        not_listed: 0,
        registry_issue: None,
        local: String::new(),
        sinai_kept: None,
        hosted: Vec::new(),
    }
}

fn facts<'a>(mind: bool, answering: &'a Answering, catalog: Option<&'a Catalog>, knocked: &'a HashMap<String, bool>, port: &'a dyn Fn(u16) -> Option<bool>) -> Facts<'a> {
    Facts { mind, answering, catalog, knocked, port, ears: EarsNow::NotRunning }
}

#[test]
fn each_card_says_its_model_where_it_runs_its_state_and_what_uses_it() {
    let dir = PathBuf::from("models");
    let cat = catalog(&dir);
    let knocked = HashMap::from([("lm".to_string(), true)]);
    let down = |_: u16| Some(false);
    let none = Choices::default();
    // the public build: nobody answers until a model is chosen
    let card = card(Part::Mind, &none, &facts(false, &Answering::Nobody, Some(&cat), &knocked, &down));
    assert_eq!((card.model.as_str(), card.level), ("None chosen", Level::Off));
    let own = Answering::Own(Pick::Endpoint("lm".into()));
    let c = super::card(Part::Mind, &none, &facts(false, &own, Some(&cat), &knocked, &down));
    assert_eq!((c.model.as_str(), c.runs.as_str(), c.level), ("LM Studio", "on this PC (127.0.0.1:1234)", Level::Up));
    assert!(c.used_by.starts_with("Sinai's page"));
    // the official build: Sinai's mind, as its service answers
    let up = |p: u16| Some(p == 8179);
    let c = super::card(Part::Mind, &none, &facts(true, &Answering::Mind, Some(&cat), &knocked, &up));
    assert_eq!((c.model.as_str(), c.level, c.state.as_str()), ("Sinai's mind", Level::Up, "Running"));
    assert!(c.used_by.contains("until Alelyon closes"), "{}", c.used_by);
    // eyes: off by default, and honest about what uses the choice in each build
    let c = super::card(Part::Eyes, &none, &facts(false, &Answering::Nobody, Some(&cat), &knocked, &down));
    assert_eq!((c.model.as_str(), c.level), ("Off", Level::Off));
    assert!(c.used_by.starts_with("Nothing in this build uses it"), "{}", c.used_by);
    let mut chosen = Choices::default();
    chosen.set(Part::Eyes, Some(Choice::Gguf("qwen".into())));
    let c = super::card(Part::Eyes, &chosen, &facts(true, &Answering::Mind, Some(&cat), &knocked, &down));
    assert_eq!((c.model.as_str(), c.runs.as_str()), ("qwen", "on this PC (llama.cpp)"));
    assert!(c.used_by.contains("does not read this choice yet") && c.state.contains("opens when it looks"), "{c:?}");
    // voice: the loop's own, in words for each build
    let c = super::card(Part::Voice, &none, &facts(false, &Answering::Nobody, None, &knocked, &down));
    assert_eq!((c.model.as_str(), c.state.as_str()), ("Sinai's loop's own", "Not in this build."));
}

#[test]
fn the_ears_card_says_when_a_choice_waits_for_the_next_start() {
    let knocked = HashMap::new();
    let down = |_: u16| Some(false);
    let mut chosen = Choices::default();
    chosen.set(Part::Ears, Some(Choice::SpeechFile("ggml-base.en.bin".into())));
    let mut f = facts(false, &Answering::Nobody, None, &knocked, &down);
    f.ears = EarsNow::Running { recognizer: "ready".into(), model: "ggml-large-v3-turbo-q5_0.bin".into() };
    let c = card(Part::Ears, &chosen, &f);
    assert_eq!(c.model, "ggml-base.en.bin");
    assert!(c.state.contains("next starts from Alelyon"), "{}", c.state);
    f.ears = EarsNow::Running { recognizer: "ready".into(), model: "ggml-base.en.bin".into() };
    assert_eq!(card(Part::Ears, &chosen, &f).state, "Running ggml-base.en.bin");
    f.ears = EarsNow::NotRunning;
    assert_eq!(card(Part::Ears, &chosen, &f).level, Level::Off);
}

#[test]
fn a_choice_the_part_cannot_use_is_refused_with_why() {
    let dir = PathBuf::from("models");
    let cat = catalog(&dir);
    let mut s = State::at(None, None);
    assert!(s.cannot_choose(Part::Mind, &Choice::SinaiMind, Some(&cat), false).is_some(), "no Sinai's mind in this build");
    assert!(s.cannot_choose(Part::Mind, &Choice::SinaiMind, Some(&cat), true).is_none());
    assert!(s.cannot_choose(Part::Voice, &Choice::Gguf("qwen".into()), Some(&cat), true).is_some(), "a voice is not a GGUF file");
    assert!(s.cannot_choose(Part::Eyes, &Choice::Gguf("missing".into()), Some(&cat), false).is_some(), "not in the folder");
    assert!(s.cannot_choose(Part::Eyes, &Choice::Gguf("qwen".into()), Some(&cat), false).is_none());
    assert!(s.cannot_choose(Part::Eyes, &Choice::Cloud { provider: "p".into(), model: "m".into(), agreed: Some("x".into()) }, Some(&cat), false).is_some());
    assert!(s.cannot_choose(Part::Ears, &Choice::SpeechFile("ggml-base.en.bin".into()), None, false).is_some(), "not listed");
    s.snapshot = Some(Snapshot {
        speech: vec![SpeechFile { name: "ggml-base.en.bin".into(), path: dir.join("ggml-base.en.bin"), size: 1 }],
        ..Snapshot::default()
    });
    assert!(s.cannot_choose(Part::Ears, &Choice::SpeechFile("ggml-base.en.bin".into()), None, false).is_none());
    // a window that keeps no preferences says so, and nothing is written
    let then = s.update(Msg::Choose(Part::Eyes, Some(Choice::Gguf("qwen".into()))), Some(&cat), false);
    assert!(matches!(then, Then::Nothing));
    assert!(matches!(&s.said, Some(Err(w)) if w.contains("keeps no preferences")), "{:?}", s.said);
    assert!(!s.working);
}

#[test]
fn a_kept_choice_changes_the_cards_and_the_picker_opens_and_closes() {
    let mut s = State::at(None, None);
    s.snapshot = Some(Snapshot::default());
    assert!(matches!(s.update(Msg::Open(Part::Ears), None, false), Then::Nothing), "already read: nothing to read");
    assert_eq!(s.open, Some(Part::Ears));
    let then = s.update(Msg::Chosen(Part::Eyes, Some(Choice::Gguf("qwen".into())), Ok(())), None, false);
    assert!(matches!(then, Then::Chosen(Part::Eyes, Some(Choice::Gguf(_)))));
    assert_eq!(s.choices().used(Part::Eyes), Some(&Choice::Gguf("qwen".into())));
    let then = s.update(Msg::Chosen(Part::Eyes, None, Err("disk full".into())), None, false);
    assert!(matches!(then, Then::Nothing));
    assert_eq!(s.said, Some(Err("disk full".into())));
    assert_eq!(s.choices().used(Part::Eyes), Some(&Choice::Gguf("qwen".into())), "a write that failed changes nothing");
    assert!(matches!(s.update(Msg::AddModels, None, false), Then::OpenModels));
    assert_eq!(s.open, None);
    assert!(Part::ALL.iter().all(|p| Part::ALL.iter().filter(|q| q.key() == p.key()).count() == 1), "one key each");
}

#[test]
fn every_part_card_on_the_overview_is_a_service_or_the_voice() {
    // the parts a service's card stands for: every part but the voice has one; the loop and the engine are services
    let mut parts: Vec<Part> = crate::probe::SERVICES.iter().filter_map(|s| Part::of_port(s.port)).collect();
    parts.dedup();
    for part in [Part::Mind, Part::Eyes, Part::Ears, Part::Embeddings] {
        assert!(parts.contains(&part), "{part:?}");
    }
    assert_eq!(Part::of_port(8180), None, "Sinai's loop is a service, not a model");
    assert_eq!(Part::of_port(8186), None, "the speech engine's socket is a service");
}
