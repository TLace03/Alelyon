//! What a call of one of Sinai's parts on its cloud model checks before anything is sent, for a caller beside the
//! window (a build's own service for Sinai's loop): the window's own rules, read from the same records, so the two
//! cannot disagree and no second way to agree exists.
//!
//! A part's cloud model is called only when:
//! - its entry in the preferences is a cloud model with its `"agreed"` stamp, for a provider the part offers and a
//!   free model (on Cloudflare, one the free-allocation meter has a rate for): the Agree step is the only writer;
//! - a model that asks each account to accept a licence first (the eyes' Llama 3.2 Vision on Cloudflare) has that
//!   acceptance recorded from this PC (`cloud-licences.json`, written by the eyes' Agree step);
//! - the part is not paused at the provider's free limit (`cloud-pauses.json`, read as it is on disk now), and, on
//!   Cloudflare, the account's metered count has not reached the meter's stop;
//! - the provider is connected (its key, and Cloudflare's account id, are kept where lattice-core finds keys).
//!
//! The call itself goes through lattice-core's `hosted::parts` with the [`Reach`] [`admit`] returns
//! (`Reach::found`, which attaches Cloudflare's meter), so it is counted and refused exactly as the window's calls are.
//! A limit met during a call is kept with [`note_limit`], so the window's cards show the same pause.

use lattice_core::hosted::parts::{CLOUDFLARE, Cloud, Reach};

use crate::sinai_parts::cloud;
use crate::sinai_parts::{Choice, Choices};

pub use crate::sinai_parts::Part;

/// The part whose preferences key is `key` (`"eyes"`, `"embeddings"`, ...).
pub fn part(key: &str) -> Option<Part> {
    Part::ALL.into_iter().find(|p| p.key() == key)
}

/// Why a part's cloud model is not called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// No agreed cloud model is kept for the part, in words.
    NotAgreed(String),
    /// The model kept is not a free model the part offers there, in words.
    NotFree(String),
    /// The model asks each account to accept a licence, and no acceptance is recorded from this PC.
    Licence(String),
    /// The part is paused at its provider's free limit until this time (seconds since 1970).
    Paused { until: i64 },
    /// The provider is not connected, in words.
    NotConnected(String),
}

impl Refused {
    /// A short, stable name for the reason.
    pub fn reason(&self) -> &'static str {
        match self {
            Refused::NotAgreed(_) => "not-agreed",
            Refused::NotFree(_) => "not-free",
            Refused::Licence(_) => "licence",
            Refused::Paused { .. } => "paused",
            Refused::NotConnected(_) => "not-connected",
        }
    }

    /// The reason in a sentence, with the time a pause ends in this PC's time.
    pub fn words(&self, now: i64) -> String {
        match self {
            Refused::NotAgreed(s) | Refused::NotFree(s) | Refused::Licence(s) | Refused::NotConnected(s) => s.clone(),
            Refused::Paused { until } => cloud::back_words(*until, now),
        }
    }
}

/// A part's cloud model, admitted for a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admitted {
    pub part: Part,
    pub cloud: &'static Cloud,
    pub model: String,
}

/// The rule, over what is read: `choice` is the part's entry as kept; `licence(model)` whether that model's licence
/// was accepted on the connected account; `paused(provider)` when the part's pause there ends; `connected(cloud)`
/// whether its key is kept.
pub fn decide(
    part: Part,
    choice: Option<&Choice>,
    licence: &dyn Fn(&str) -> bool,
    paused: &dyn Fn(&str) -> Option<i64>,
    connected: &dyn Fn(&Cloud) -> bool,
) -> Result<Admitted, Refused> {
    let name = part.title().to_lowercase();
    let (provider, model) = match choice {
        None => return Err(Refused::NotAgreed(format!("No cloud model is chosen for Sinai's {name}."))),
        Some(Choice::Cloud { agreed: None, .. }) => {
            return Err(Refused::NotAgreed(format!(
                "The cloud model for Sinai's {name} is not agreed: nothing is sent before Agree in its picker."
            )));
        }
        Some(Choice::Cloud { provider, model, agreed: Some(_) }) => (provider.as_str(), model.as_str()),
        Some(_) => return Err(Refused::NotAgreed(format!("Sinai's {name} is not given a cloud model."))),
    };
    let Some(offer) = cloud::offer(part, provider) else {
        return Err(Refused::NotAgreed(format!("Sinai's {name} is not offered on {provider} in this version.")));
    };
    if !cloud::allowed(part, provider, model) {
        return Err(Refused::NotFree(format!(
            "{model} is not a free model Sinai's {name} offers on {}: Alelyon calls free models only.",
            offer.cloud.label
        )));
    }
    if offer.licence.is_some() && !licence(model) {
        return Err(Refused::Licence(format!(
            "{model} asks each account to accept its licence, and no acceptance is recorded from this PC: accept it in \
             the picker for Sinai's {name}."
        )));
    }
    if let Some(until) = paused(provider) {
        return Err(Refused::Paused { until });
    }
    if !connected(offer.cloud) {
        return Err(Refused::NotConnected(format!(
            "Connect {} again in the picker for Sinai's {name}: no key is kept for it.",
            offer.cloud.label
        )));
    }
    Ok(Admitted { part, cloud: offer.cloud, model: model.to_string() })
}

/// [`decide`] over the records as they are now (the preferences, the licences, the pauses, the meter and the keys),
/// with the provider reached by `Reach::found`, which attaches Cloudflare's meter.
pub fn admit(part: Part, now: i64) -> Result<(Admitted, Reach), Refused> {
    let prefs = if cfg!(test) { None } else { crate::prefs::path() };
    let choices = Choices::read(prefs.as_deref());
    let pauses = cloud::beside_prefs("cloud-pauses.json");
    // the licences recorded are Cloudflare's (per account and model), the only provider here that asks for one
    let licence = |model: &str| {
        CLOUDFLARE.account_name.and_then(cloud::found).is_some_and(|account| cloud::licence_accepted(account.expose(), model))
    };
    let paused = |provider: &str| {
        let kept = pauses.as_deref().and_then(|p| cloud::paused_at(p, part, provider, now));
        kept.or_else(|| (provider == CLOUDFLARE.id).then(|| cloud::cloudflare_spent(now)).flatten())
    };
    let admitted = decide(part, choices.get(part), &licence, &paused, &|c| cloud::connected(c))?;
    let reach = cloud::reach(admitted.cloud).map_err(Refused::NotConnected)?;
    Ok((admitted, reach))
}

/// Keep `part` paused on `provider` until `until`, beside the preferences, where the window's cards read it.
pub fn note_limit(part: Part, provider: &str, until: i64, now: i64) -> Result<(), String> {
    let path = cloud::beside_prefs("cloud-pauses.json").ok_or("This PC keeps no preferences, so the pause was not kept.")?;
    cloud::pause_at(&path, part, provider, until, now)
}

/// Whether any of `parts` has an agreed cloud model in the preferences as they are now.
pub fn agreed_any(parts: &[Part]) -> bool {
    let prefs = if cfg!(test) { None } else { crate::prefs::path() };
    let choices = Choices::read(prefs.as_deref());
    parts.iter().any(|p| matches!(choices.used(*p), Some(Choice::Cloud { .. })))
}

/// Seconds since 1970, now.
pub fn now() -> i64 {
    cloud::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agreed(provider: &str, model: &str) -> Choice {
        Choice::Cloud { provider: provider.into(), model: model.into(), agreed: Some("2026-10-10T12:00:00Z".into()) }
    }

    fn yes(_: &str) -> bool {
        true
    }

    fn never(_: &str) -> Option<i64> {
        None
    }

    fn connected(_: &Cloud) -> bool {
        true
    }

    #[test]
    fn an_agreed_free_model_the_part_offers_is_admitted() {
        let a = decide(Part::Embeddings, Some(&agreed("cloudflare", "@cf/baai/bge-m3")), &yes, &never, &connected).unwrap();
        assert_eq!((a.part, a.cloud.id, a.model.as_str()), (Part::Embeddings, "cloudflare", "@cf/baai/bge-m3"));
        let a = decide(Part::Voice, Some(&agreed("cloudflare", "@cf/myshell-ai/melotts")), &yes, &never, &connected).unwrap();
        assert_eq!(a.cloud.id, "cloudflare");
    }

    #[test]
    fn no_agreement_no_call() {
        let none = decide(Part::Eyes, None, &yes, &never, &connected).unwrap_err();
        assert_eq!(none.reason(), "not-agreed");
        let unagreed = Choice::Cloud { provider: "cloudflare".into(), model: "@cf/baai/bge-m3".into(), agreed: None };
        let e = decide(Part::Embeddings, Some(&unagreed), &yes, &never, &connected).unwrap_err();
        assert_eq!(e.reason(), "not-agreed");
        assert!(e.words(0).contains("not agreed"), "{e:?}");
        let local = Choice::Gguf("bge-small".into());
        assert_eq!(decide(Part::Embeddings, Some(&local), &yes, &never, &connected).unwrap_err().reason(), "not-agreed");
        // a provider the part does not offer
        let e = decide(Part::Voice, Some(&agreed("groq", "playai-tts")), &yes, &never, &connected).unwrap_err();
        assert_eq!(e.reason(), "not-agreed");
    }

    #[test]
    fn a_paid_or_unmetered_model_is_refused_even_when_agreed() {
        // OpenRouter's paid variant of a model the mind offers
        let e = decide(Part::Mind, Some(&agreed("openrouter", "meta-llama/llama-3.3-70b-instruct")), &yes, &never, &connected);
        assert_eq!(e.unwrap_err().reason(), "not-free");
        // a Cloudflare model the meter has no rate for
        let e = decide(Part::Embeddings, Some(&agreed("cloudflare", "@cf/unknown/model")), &yes, &never, &connected);
        assert_eq!(e.unwrap_err().reason(), "not-free");
    }

    #[test]
    fn the_eyes_on_cloudflare_need_the_licence_recorded() {
        let eyes = agreed("cloudflare", "@cf/meta/llama-3.2-11b-vision-instruct");
        let e = decide(Part::Eyes, Some(&eyes), &|_| false, &never, &connected).unwrap_err();
        assert_eq!(e.reason(), "licence");
        assert!(decide(Part::Eyes, Some(&eyes), &yes, &never, &connected).is_ok());
        // a model without a licence step needs no record
        let flash = agreed("gemini", "gemini-2.5-flash");
        assert!(decide(Part::Eyes, Some(&flash), &|_| false, &never, &connected).is_ok());
    }

    #[test]
    fn a_pause_and_a_missing_key_refuse() {
        let emb = agreed("cloudflare", "@cf/baai/bge-m3");
        let e = decide(Part::Embeddings, Some(&emb), &yes, &|p| (p == "cloudflare").then_some(1_000), &connected).unwrap_err();
        assert_eq!(e, Refused::Paused { until: 1_000 });
        assert!(e.words(0).starts_with("Free limit reached, back at"));
        let e = decide(Part::Embeddings, Some(&emb), &yes, &never, &|_| false).unwrap_err();
        assert_eq!(e.reason(), "not-connected");
    }

    #[test]
    fn a_limit_is_kept_on_disk_beside_what_another_process_kept() {
        let dir = std::env::temp_dir().join(format!("centcom-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("cloud-pauses.json");
        cloud::pause_at(&path, Part::Ears, "groq", 5_000, 100).unwrap();
        cloud::pause_at(&path, Part::Voice, "cloudflare", 4_000, 100).unwrap();
        // an earlier time does not shorten a pause; both parts' pauses are kept
        cloud::pause_at(&path, Part::Voice, "cloudflare", 3_000, 100).unwrap();
        assert_eq!(cloud::paused_at(&path, Part::Voice, "cloudflare", 200), Some(4_000));
        assert_eq!(cloud::paused_at(&path, Part::Ears, "groq", 200), Some(5_000));
        assert_eq!(cloud::paused_at(&path, Part::Voice, "groq", 200), None, "another provider is not paused");
        assert_eq!(cloud::paused_at(&path, Part::Voice, "cloudflare", 4_000), None, "a pause ends at its time");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parts_are_named_by_their_preferences_key() {
        assert_eq!(part("eyes"), Some(Part::Eyes));
        assert_eq!(part("embeddings"), Some(Part::Embeddings));
        assert_eq!(part("hands"), None);
    }
}
