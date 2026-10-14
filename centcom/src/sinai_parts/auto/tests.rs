//! Auto configure: the shipped catalogue holds to its rules, the validator refuses what breaks them, the chooser's
//! rules, the dials and paid allowances as kept, and the way back. Nothing here reaches a network, a key or a real
//! preferences file.

use std::collections::{HashMap, HashSet};

use lattice_core::hosted::spend::{Price, Spend};
use serde_json::{Value, json};

use super::catalogue::{self, Effort, Source};
use super::choose::{self, Inputs, Listed, Machine, Now, Pick, Row, Tier};
use super::*;
use crate::sinai_parts::{Choice, Choices, Part};

const GB: u64 = 1_000_000_000;

fn cat() -> Catalogue {
    catalogue::shipped().unwrap_or_else(|wrong| panic!("the shipped catalogue is refused: {wrong:#?}"))
}

#[test]
fn the_shipped_catalogue_holds_to_its_rules() {
    let c = cat();
    assert_eq!(c.version, 1);
    assert!(!c.candidates.is_empty());
    for cand in &c.candidates {
        let source = c.scales.get(&cand.scale).map(|s| &s.source).expect("every rank has a source");
        match source {
            Source::Benchmark(url) => assert!(url.starts_with("https://"), "{url}"),
            Source::Measurement(file) => {
                let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
                assert!(crate_dir.join(file).is_file(), "a measurement file is in this crate: {file}");
            }
        }
        assert!(!cand.basis.is_empty());
        let lower = format!("{} {}", cand.id, cand.name).to_lowercase();
        assert!(!lower.contains("alelyon") && !lower.contains("sinai"), "no Alelyon model: {lower}");
    }
    // each part ranks on one scale, and the voice has no candidate yet (its entries wait for a source)
    for part in Part::ALL {
        let scales: HashSet<&str> = c.of(part).map(|x| x.scale.as_str()).collect();
        assert!(scales.len() <= 1, "{part:?}: {scales:?}");
    }
    assert_eq!(c.of(Part::Voice).count(), 0);
    assert!(c.todo.iter().any(|t| t.part == "voice") && c.todo.iter().any(|t| t.part == "ears"), "the entries that need a source are listed");
    // every eyes model on this PC comes with its projector
    for e in c.of(Part::Eyes) {
        assert!(e.local.as_ref().unwrap().files.iter().any(|f| f.role == "projector"));
    }
}

fn minimal(candidate: Value) -> String {
    json!({
        "schema": "sinai-parts-catalogue", "version": 1, "as_of": "2026-10-14",
        "scales": {"s": {"part": "mind", "title": "a scale", "source": {"kind": "benchmark", "url": "https://example.org/paper"}}},
        "candidates": [candidate],
        "todo": [],
    })
    .to_string()
}

fn good() -> Value {
    json!({
        "id": "m-1", "part": "mind", "name": "M", "params_b": 1.0,
        "modalities": {"in": ["text"], "out": ["text"]},
        "licence": {"id": "Apache-2.0", "open_weights": true, "url": "https://example.org/l"},
        "efforts": ["light"], "quality": {"scale": "s", "rank": 1, "basis": "b"},
        "local": {"repo": "o/r", "revision": "main", "files": [{"path": "m.gguf", "role": "model"}]},
    })
}

#[test]
fn the_validator_refuses_a_rank_without_a_source_and_an_alelyon_model() {
    assert!(catalogue::parse(&minimal(good())).is_ok());
    let refused = |patch: &dyn Fn(&mut Value), says: &str| {
        let mut c = good();
        patch(&mut c);
        let wrong = catalogue::parse(&minimal(c)).expect_err(says);
        assert!(wrong.iter().any(|w| w.contains(says)), "{says}: {wrong:?}");
    };
    refused(&|c| c["quality"]["scale"] = json!("nowhere"), "has no source");
    refused(&|c| c["quality"]["basis"] = json!(""), "needs a basis");
    refused(&|c| c["quality"]["rank"] = json!(0), "whole number");
    refused(&|c| c["name"] = json!("Alelyon Seed"), "Alelyon model");
    refused(&|c| c["local"]["repo"] = json!("alelyon/mind"), "Alelyon model");
    refused(&|c| c["local"]["files"][0]["path"] = json!("../m.gguf"), "plain paths");
    refused(&|c| c["local"]["repo"] = json!("not a repo"), "Hugging Face name");
    refused(
        &|c| {
            c.as_object_mut().unwrap().remove("local");
        },
        "runs nowhere",
    );
    refused(&|c| c["cloud"] = json!([{"provider": "elsewhere", "model": "x"}]), "not one Sinai's parts run on");
    refused(&|c| c["efforts"] = json!(["heroic"]), "efforts");
    refused(
        &|c| {
            c["part"] = json!("eyes");
        },
        "belongs to the mind",
    );
    // a second scale for one part, and an http source, are refused
    let mut two = serde_json::from_str::<Value>(&minimal(good())).unwrap();
    two["scales"]["t"] = json!({"part": "mind", "title": "another", "source": {"kind": "benchmark", "url": "https://example.org/2"}});
    assert!(catalogue::parse(&two.to_string()).unwrap_err().iter().any(|w| w.contains("two scales")));
    let mut http = serde_json::from_str::<Value>(&minimal(good())).unwrap();
    http["scales"]["s"]["source"]["url"] = json!("http://example.org/paper");
    assert!(catalogue::parse(&http.to_string()).is_err());
    // two candidates may not share a rank: ranks order, they do not tie
    let mut tie = serde_json::from_str::<Value>(&minimal(good())).unwrap();
    let mut second = good();
    second["id"] = json!("m-2");
    tie["candidates"].as_array_mut().unwrap().push(second);
    assert!(catalogue::parse(&tie.to_string()).unwrap_err().iter().any(|w| w.contains("given twice")));
}

fn sizes() -> HashMap<String, Result<u64, String>> {
    [
        ("qwen3-4b", 2_500_000_000u64),
        ("qwen3-8b", 5 * GB),
        ("qwen3-14b", 9 * GB),
        ("qwen3-32b", 20 * GB),
        ("whisper-base", 148_000_000),
        ("whisper-small", 488_000_000),
        ("whisper-medium", 1_530_000_000),
        ("qwen3-embedding-0.6b", 640_000_000),
        ("qwen3-embedding-4b", 2_500_000_000),
        ("qwen2.5-vl-3b", 2_800_000_000),
        ("qwen2.5-vl-7b", 5_500_000_000),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), Ok(v)))
    .collect()
}

fn inputs(c: &Catalogue, effort: Effort, tier: Tier, budget: Option<u64>) -> Inputs<'_> {
    Inputs {
        catalogue: c,
        effort,
        tier,
        machine: Machine { budget, basis: "test".into(), disk_free: Some(500 * GB) },
        sizes: sizes(),
        lists: HashMap::new(),
        connected: HashSet::new(),
        eyes_on: false,
        now: HashMap::new(),
    }
}

fn row(p: &choose::Proposal, part: Part) -> &Row {
    p.rows.iter().find(|r| r.parts.contains(&part)).unwrap()
}

fn id(r: &Row) -> &str {
    match &r.pick {
        Pick::Local { id, .. } | Pick::Cloud { id, .. } => id,
        _ => "",
    }
}

fn price(i: f64, o: f64) -> Price {
    Price::known(i, o).unwrap()
}

#[test]
fn this_pc_only_picks_the_best_that_fits_and_the_parts_share_the_memory() {
    let c = cat();
    let p = choose::propose(&inputs(&c, Effort::Balanced, Tier::ThisPc, Some(12 * GB)));
    // the mind first: Qwen3 14B (rank 3) fits 12 GB with a fifth to spare; then the ears' small model; the embeddings
    // no longer fit beside them
    assert_eq!(id(row(&p, Part::Mind)), "qwen3-14b");
    assert_eq!(id(row(&p, Part::Ears)), "whisper-small", "medium does not fit beside the mind");
    assert_eq!(row(&p, Part::Embeddings).pick, Pick::Nothing);
    assert!(row(&p, Part::Embeddings).why.contains("does not fit"), "{}", row(&p, Part::Embeddings).why);
    assert_eq!(row(&p, Part::Eyes).pick, Pick::Off, "the eyes stay off until turned on");
    assert_eq!(row(&p, Part::Voice).pick, Pick::Nothing, "no ranked voice yet");
    assert_eq!(p.monthly, 0.0);
    assert_eq!(p.download, 9 * GB + 488_000_000);
    assert!(p.rows.iter().all(|r| !matches!(r.pick, Pick::Cloud { .. })), "nothing leaves this PC");
    // Light: the small mind
    let light = choose::propose(&inputs(&c, Effort::Light, Tier::ThisPc, Some(12 * GB)));
    assert_eq!(id(row(&light, Part::Mind)), "qwen3-4b");
    // a budget that is not measured lets nothing run here, and says so
    let unmeasured = choose::propose(&inputs(&c, Effort::Balanced, Tier::ThisPc, None));
    assert_eq!(row(&unmeasured, Part::Mind).pick, Pick::Nothing);
    assert!(row(&unmeasured, Part::Mind).why.contains("not measured"), "{}", row(&unmeasured, Part::Mind).why);
    // the eyes, turned on, take their own row
    let mut on = inputs(&c, Effort::Light, Tier::ThisPc, Some(24 * GB));
    on.eyes_on = true;
    let p = choose::propose(&on);
    assert_eq!(id(row(&p, Part::Eyes)), "qwen2.5-vl-3b");
    let Pick::Local { files, .. } = &row(&p, Part::Eyes).pick else { panic!() };
    assert_eq!(files.len(), 2, "with its projector");
}

#[test]
fn when_nothing_suited_to_the_effort_fits_the_best_other_candidate_that_does_is_proposed_and_said() {
    let c = cat();
    // Deep on 14.4 GB: the 14B mind and the medium ears leave no room for the 4B embeddings (Deep's only one)
    let p = choose::propose(&inputs(&c, Effort::Deep, Tier::ThisPc, Some(14_400_000_000)));
    assert_eq!(id(row(&p, Part::Mind)), "qwen3-14b");
    assert_eq!(id(row(&p, Part::Ears)), "whisper-medium");
    let emb = row(&p, Part::Embeddings);
    assert_eq!(id(emb), "qwen3-embedding-0.6b");
    assert!(emb.why.contains("Nothing suited to deep fits"), "{}", emb.why);
    assert!(emb.why.contains("Source: https://arxiv.org/abs/2506.05176"), "the row cites its source: {}", emb.why);
}

fn openrouter(rows: &[(&str, Price)]) -> Listed {
    Listed::Read(rows.iter().map(|(id, p)| (id.to_string(), *p)).collect())
}

#[test]
fn this_pc_is_preferred_when_close_and_the_cloud_only_when_clearly_better() {
    let c = cat();
    let free = price(0.0, 0.0);
    let list = openrouter(&[("qwen/qwen3-14b:free", free), ("qwen/qwen3-235b-a22b:free", free)]);
    // Balanced on 7 GB: only the 8B fits here (rank 2); the cloud's 14B (rank 3) is within a step: this PC
    let mut i = inputs(&c, Effort::Balanced, Tier::FreeCloud, Some(7 * GB));
    i.lists.insert("openrouter/mind".into(), list.clone());
    let p = choose::propose(&i);
    assert_eq!(id(row(&p, Part::Mind)), "qwen3-8b");
    assert!(row(&p, Part::Mind).why.contains("This PC is preferred"), "{}", row(&p, Part::Mind).why);
    // Deep on 12 GB: 14B here (rank 3) against the flagship in the cloud (rank 5): the cloud, free, thinking on
    let mut i = inputs(&c, Effort::Deep, Tier::FreeCloud, Some(12 * GB));
    i.lists.insert("openrouter/mind".into(), list);
    let p = choose::propose(&i);
    let mind = row(&p, Part::Mind);
    assert!(matches!(&mind.pick, Pick::Cloud { model, price, .. } if model == "qwen/qwen3-235b-a22b:free" && price.is_free()), "{:?}", mind.pick);
    assert!(mind.thinking && mind.connect, "thinking on; OpenRouter is connected on its card after Agree");
    assert_eq!(p.monthly, 0.0);
    // the same at This PC only never leaves the PC
    let mut i = inputs(&c, Effort::Deep, Tier::ThisPc, Some(12 * GB));
    i.lists.insert("openrouter/mind".into(), openrouter(&[("qwen/qwen3-235b-a22b:free", free)]));
    assert_eq!(id(row(&choose::propose(&i), Part::Mind)), "qwen3-14b");
}

#[test]
fn a_paid_model_needs_a_paid_tier_a_known_price_and_room_under_the_cap() {
    let c = cat();
    let paid = price(0.13, 0.6);
    let list = openrouter(&[("qwen/qwen3-235b-a22b", paid), ("qwen/qwen3-32b", price(0.1, 0.3))]);
    let with = |tier: Tier, list: Listed| {
        let mut i = inputs(&c, Effort::Deep, tier, Some(GB));
        i.lists.insert("openrouter/mind".into(), list);
        choose::propose(&i)
    };
    // typical use: 2.7M tokens sent and 0.54M written a month
    let p = with(Tier::Cap(5.0), list.clone());
    let mind = row(&p, Part::Mind);
    let expected = (2_700_000.0 * 0.13 + 540_000.0 * 0.6) / 1e6;
    assert!(matches!(&mind.pick, Pick::Cloud { model, monthly, .. } if model == "qwen/qwen3-235b-a22b" && (monthly - expected).abs() < 1e-9), "{:?}", mind.pick);
    assert!(mind.pick.paid() && (p.monthly - expected).abs() < 1e-9);
    // a cap below the flagship's month: the next best that fits the cap
    let p = with(Tier::Cap(0.5), list.clone());
    assert!(matches!(&row(&p, Part::Mind).pick, Pick::Cloud { model, .. } if model == "qwen/qwen3-32b"));
    // a free tier never picks a paid model, even when listed
    let p = with(Tier::FreeCloud, list);
    assert_eq!(row(&p, Part::Mind).pick, Pick::Nothing);
    // an unknown price is never offered: the paid model is not on the priced list
    let p = with(Tier::Cap(20.0), openrouter(&[]));
    assert_eq!(row(&p, Part::Mind).pick, Pick::Nothing);
    // a custom tier with no number allows nothing paid
    assert_eq!(Tier::Cap(0.0).cap(), None);
}

#[test]
fn a_choice_the_catalogue_cannot_rank_is_kept_and_rows_for_one_model_merge() {
    let c = cat();
    let mut i = inputs(&c, Effort::Balanced, Tier::ThisPc, Some(12 * GB));
    i.now.insert(Part::Mind, Now::Unranked("Sinai's mind".into()));
    let p = choose::propose(&i);
    assert_eq!(row(&p, Part::Mind).pick, Pick::Keep("Sinai's mind".into()));
    assert_eq!(super::now_of(&c, Part::Mind, Some(&Choice::SinaiMind), "Sinai's mind"), Now::Unranked("Sinai's mind".into()));
    assert_eq!(super::now_of(&c, Part::Mind, Some(&Choice::Gguf("Qwen3-8B-Q4_K_M".into())), "x"), Now::Open, "a catalogue model is ranked");
    let cloud = Choice::Cloud { provider: "openrouter".into(), model: "qwen/qwen3-8b:free".into(), agreed: None };
    assert_eq!(super::now_of(&c, Part::Mind, Some(&cloud), "x"), Now::Open);
    // one model chosen for two parts is one row covering both
    let pick = Pick::Cloud { id: "x".into(), name: "X".into(), provider: "openrouter".into(), model: "m".into(), price: price(0.0, 0.0), monthly: 0.0 };
    let rows = vec![
        Row { parts: vec![Part::Mind], pick: pick.clone(), why: String::new(), thinking: true, connect: false },
        Row { parts: vec![Part::Eyes], pick, why: String::new(), thinking: false, connect: false },
    ];
    let merged = choose::merge(rows);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].parts, [Part::Mind, Part::Eyes]);
    // a proposal differs from what is in effect only where a part's model would change
    let p = choose::propose(&inputs(&c, Effort::Light, Tier::ThisPc, Some(12 * GB)));
    let mind = Choice::Gguf("Qwen3-4B-Q4_K_M".into());
    let ears = Choice::SpeechFile("ggml-small.bin".into());
    let emb = Choice::Gguf("Qwen3-Embedding-0.6B-Q8_0".into());
    assert_eq!(p.choices(), vec![(Part::Mind, mind.clone()), (Part::Ears, ears.clone()), (Part::Embeddings, emb.clone())]);
    let same = |part: Part| match part {
        Part::Mind => Some(mind.clone()),
        Part::Ears => Some(ears.clone()),
        Part::Embeddings => Some(emb.clone()),
        _ => None,
    };
    assert!(!choose::differs(&p, &same));
    assert!(choose::differs(&p, &|_| None));
}

#[test]
fn the_dials_and_a_paid_allowance_are_kept_and_hold_only_at_a_paid_tier() {
    assert_eq!(custom_cap("12.5"), Some(12.5));
    assert_eq!(custom_cap("$7"), Some(7.0));
    for bad in ["", "0", "-3", "abc", "1001", "NaN"] {
        assert_eq!(custom_cap(bad), None, "{bad}");
    }
    let mut s = Settings { effort: Effort::Deep, budget: Budget::Custom, custom_usd: Some(12.5), ..Settings::default() };
    s.paid.insert(Part::Mind, PaidTerms { provider: "openrouter".into(), model: "qwen/qwen3-235b-a22b".into(), price: price(0.13, 0.6), agreed: "2026-10-14T10:00:00".into() });
    let back = Settings::from_map(&s.to_map());
    assert_eq!(back, s, "kept and read back");
    assert_eq!(back.tier(), Tier::Cap(12.5));
    assert!(back.paid_for(Part::Mind, "openrouter", "qwen/qwen3-235b-a22b").is_some());
    assert!(back.paid_for(Part::Mind, "openrouter", "qwen/qwen3-32b").is_none(), "only the very model agreed");
    assert!(back.paid_for(Part::Eyes, "openrouter", "qwen/qwen3-235b-a22b").is_none());
    let free = Settings { budget: Budget::FreeCloud, ..back.clone() };
    assert!(free.paid_for(Part::Mind, "openrouter", "qwen/qwen3-235b-a22b").is_none(), "a free tier allows no paid model");
    assert_eq!(Settings::from_map(&Map::new()), Settings::default());
    assert_eq!(Settings::default().budget, Budget::ThisPc, "the most private tier until turned");
}

#[test]
fn a_paid_choice_is_used_only_with_its_allowance_and_the_reach_carries_the_cap() {
    let dir = crate::sqlite_ro::tests::scratch("auto-paid");
    let prefs = dir.join("preferences.json");
    let ledger = dir.join("paid-spend.json");
    let model = "qwen/qwen3-235b-a22b";
    let mut s = Settings { budget: Budget::Five, ..Settings::default() };
    s.paid.insert(Part::Mind, PaidTerms { provider: "openrouter".into(), model: model.into(), price: price(0.13, 0.6), agreed: "t".into() });
    crate::prefs::save_auto(&prefs, s.to_map()).unwrap();
    crate::sinai_parts::save(&prefs, Part::Mind, Some(&Choice::Cloud { provider: "openrouter".into(), model: model.into(), agreed: Some("t".into()) })).unwrap();
    let choices = Choices::read(Some(&prefs));
    assert!(choices.used(Part::Mind).is_some(), "the agreed paid model is in effect at a paid tier");
    test_places(Some(ledger.clone()), Some(prefs.clone()));
    let cloud = lattice_core::hosted::parts::cloud("openrouter").unwrap();
    let reach = || lattice_core::hosted::parts::Reach::new(cloud, "stub-key-for-tests".into(), None, Some("http://127.0.0.1:9")).unwrap();
    let r = paid_reach(reach(), Part::Mind, model);
    let p = r.paid().expect("the allowance");
    assert_eq!((p.cap_usd, p.model.as_str(), p.part.as_str()), (5.0, model, "mind"));
    assert!(paid_reach(reach(), Part::Mind, "qwen/qwen3-32b").paid().is_none(), "another paid model is not allowed");
    assert!(paid_reach(reach(), Part::Mind, "qwen/qwen3-8b:free").paid().is_none(), "a free model needs none");
    // at the cap the card says so
    let spend = Spend::at(ledger.clone());
    let r = spend.reserve("mind", 5.0, 5.0, crate::sinai_parts::cloud::now()).unwrap();
    spend.settle(&r, 5.0).unwrap();
    let words = capped_words(Part::Mind, "openrouter", model, crate::sinai_parts::cloud::now()).unwrap();
    assert!(words.starts_with("Monthly cap reached"), "{words}");
    assert!((month_so_far(crate::sinai_parts::cloud::now()) - 5.0).abs() < 1e-9);
    // the tier turned to free: the paid choice is kept and not used, and its card says why
    let free = Settings { budget: Budget::FreeCloud, ..s };
    crate::prefs::save_auto(&prefs, free.to_map()).unwrap();
    assert!(Choices::read(Some(&prefs)).used(Part::Mind).is_none());
    assert!(paid_reach(reach(), Part::Mind, model).paid().is_none());
    assert!(capped_words(Part::Mind, "openrouter", model, crate::sinai_parts::cloud::now()).unwrap().contains("paid price tier"));
    test_places(None, None);
}

#[test]
fn agree_keeps_the_setup_before_and_back_puts_it_back_in_one_write() {
    let dir = crate::sqlite_ro::tests::scratch("auto-back");
    let prefs = dir.join("preferences.json");
    // the person's own setup before Auto configure
    crate::prefs::save_web_pages(&prefs, "yours").unwrap();
    crate::sinai_parts::save(&prefs, Part::Mind, Some(&Choice::Endpoint("lab".into()))).unwrap();
    let before = Settings::read(Some(&prefs));
    let (parts, sinai_model) = crate::prefs::load_parts_whole(&prefs);
    assert_eq!(sinai_model.as_deref(), Some("endpoint:lab"));
    let dials = Settings { effort: Effort::Deep, budget: Budget::Twenty, ..Settings::default() };
    let paid = Pick::Cloud { id: "q".into(), name: "Q".into(), provider: "openrouter".into(), model: "qwen/qwen3-235b-a22b".into(), price: price(0.13, 0.6), monthly: 0.68 };
    let free = Pick::Cloud { id: "e".into(), name: "E".into(), provider: "cloudflare".into(), model: "@cf/qwen/qwen3-embedding-0.6b".into(), price: price(0.0, 0.0), monthly: 0.0 };
    let proposal = choose::Proposal {
        rows: vec![
            Row { parts: vec![Part::Mind], pick: paid, why: String::new(), thinking: true, connect: false },
            Row { parts: vec![Part::Embeddings], pick: free, why: String::new(), thinking: false, connect: false },
        ],
        effort: Effort::Deep,
        tier: Tier::Cap(20.0),
        download: 0,
        monthly: 0.68,
    };
    let previous = Previous { parts, sinai_model, settings: before.to_map_own(), at: "2026-10-14T10:00:00".into() };
    let next = agreed_settings(&before, &dials, &proposal, previous, "2026-10-14T10:00:00");
    assert_eq!(next.paid.len(), 1, "an allowance for the paid row only");
    assert_eq!(next.paid[&Part::Mind].model, "qwen/qwen3-235b-a22b");
    assert_eq!(next.agreed.as_deref(), Some("2026-10-14T10:00:00"));
    crate::prefs::save_auto(&prefs, next.to_map()).unwrap();
    // the setup changes: the mind goes to the cloud
    crate::sinai_parts::save(&prefs, Part::Mind, Some(&Choice::Cloud { provider: "openrouter".into(), model: "qwen/qwen3-235b-a22b".into(), agreed: Some("t".into()) })).unwrap();
    assert!(Choices::read(Some(&prefs)).used(Part::Mind).is_some());
    // back: the choices, the old model key and the dials as they were, in one write; other preferences kept
    let kept = Settings::read(Some(&prefs)).previous.unwrap();
    crate::prefs::restore_parts(&prefs, kept.parts, kept.sinai_model, kept.settings).unwrap();
    assert_eq!(Choices::read(Some(&prefs)).used(Part::Mind), Some(&Choice::Endpoint("lab".into())));
    assert_eq!(crate::prefs::load_sinai_model(&prefs).as_deref(), Some("endpoint:lab"));
    let s = Settings::read(Some(&prefs));
    assert_eq!((s.budget, s.paid.len(), s.previous.is_none()), (Budget::ThisPc, 0, true), "the paid allowance went with the setup it belonged to");
    assert_eq!(crate::prefs::load_web_pages(&prefs).as_deref(), Some("yours"));
}

#[test]
fn the_spending_line_says_the_month_against_the_cap() {
    let mut s = State::with(None, Settings::default());
    assert!(view::spending_line(&s).contains("costs $0"));
    s.settings.budget = Budget::Five;
    s.month = Some(0.42);
    assert_eq!(view::spending_line(&s), "Paid cloud this month: $0.42 of your $5.00 cap.");
    s.month = Some(5.0);
    assert!(view::spending_line(&s).contains("Monthly cap reached"));
}

#[test]
fn a_proposed_cloud_row_opens_its_own_card_at_agree_and_a_paid_one_only_with_its_allowance() {
    use crate::sinai_parts::cloud::{self, Out, Stage, Step};
    let dir = crate::sqlite_ro::tests::scratch("auto-card");
    let prefs = dir.join("preferences.json");
    test_places(Some(dir.join("paid-spend.json")), Some(prefs.clone()));
    cloud::test_keys(&["OPENROUTER_API_KEY"]);
    let model = "qwen/qwen3-235b-a22b";
    // no allowance: a paid model's card does not open
    let mut flow = None;
    let _ = cloud::update(&mut flow, Step::StartWith(Part::Mind, "openrouter".into(), model.into()));
    assert!(flow.is_none(), "a paid model without an allowance is not offered");
    // a free model's card opens at its Agree; nothing is kept before it
    let _ = cloud::update(&mut flow, Step::StartWith(Part::Mind, "openrouter".into(), "qwen/qwen3-8b:free".into()));
    assert_eq!(flow.as_ref().map(|f| f.stage), Some(Stage::Agree));
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Choose(Part::Mind, Choice::Cloud { agreed: Some(_), .. })));
    // the allowance kept at a paid tier: the card opens with its price and the cap, and only its Agree keeps it
    let mut s = Settings { budget: Budget::Five, ..Settings::default() };
    s.paid.insert(Part::Mind, PaidTerms { provider: "openrouter".into(), model: model.into(), price: price(0.13, 0.6), agreed: "t".into() });
    crate::prefs::save_auto(&prefs, s.to_map()).unwrap();
    let _ = cloud::update(&mut flow, Step::StartWith(Part::Mind, "openrouter".into(), model.into()));
    let f = flow.as_ref().unwrap();
    assert_eq!((f.stage, f.paid.as_ref().map(|(t, cap)| (t.model.as_str(), *cap))), (Stage::Agree, Some((model, 5.0))));
    assert!(matches!(cloud::update(&mut flow, Step::Agree), Out::Choose(Part::Mind, Choice::Cloud { model: m, .. }) if m == model));
    // not connected: Connect first, then straight to Agree
    cloud::test_keys(&[]);
    let _ = cloud::update(&mut flow, Step::StartWith(Part::Mind, "openrouter".into(), model.into()));
    assert_eq!(flow.as_ref().map(|f| f.stage), Some(Stage::Connect));
    assert!(matches!(cloud::update(&mut flow, Step::Connected(Ok(()))), Out::Nothing));
    assert_eq!(flow.as_ref().map(|f| f.stage), Some(Stage::Agree));
    test_places(None, None);
}
