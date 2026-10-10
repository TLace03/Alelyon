//! Where the account panel's web pages open (2026-10-08): its links (Account details and Account security,
//! Create account and Can't sign in?, the Terms of Service and the Privacy Notice) open in Lattice's own browser by
//! default, each in a tab of its own (the Lattice page's `open_web`), or in the person's own browser when Settings says
//! so. Each link also has a small button that opens it in the person's own browser once. The choice is kept on this PC
//! with Alelyon's other preferences (`crate::prefs`: `"web_pages": "lattice" | "yours"` in
//! `~/.alelyon/alelyon/preferences.json`); Lattice's browser when it was never chosen or cannot be read.
//!
//! Signing in with a provider (GitHub, Google and the rest) always uses the person's own browser: an app must not run a
//! sign-in in a browser it controls (RFC 8252, OAuth for native apps), and Lattice's browser is one the agent can use.
//!
//! The person's own browser opens only an `http` or `https` address: several of these addresses come from the identity
//! service, and the shell's URL handler would hand any other scheme to whatever program claims it. Lattice's browser
//! holds every address to its own policy (the public web only).

use std::path::Path;

/// Where the account panel's web pages open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WebPages {
    /// Lattice's own browser, in a tab of its own.
    #[default]
    Lattice,
    /// The person's own (default) browser.
    Yours,
}

impl WebPages {
    pub fn key(self) -> &'static str {
        match self {
            WebPages::Lattice => "lattice",
            WebPages::Yours => "yours",
        }
    }

    fn from_key(key: &str) -> Option<WebPages> {
        match key {
            "lattice" => Some(WebPages::Lattice),
            "yours" => Some(WebPages::Yours),
            _ => None,
        }
    }
}

/// The choice kept in the preferences file at `at`: Lattice's browser when there is none, or it cannot be read.
pub fn load(at: &Path) -> WebPages {
    crate::prefs::load_web_pages(at).as_deref().and_then(WebPages::from_key).unwrap_or_default()
}

/// Keep `pages` in the preferences file at `at`, with the other preferences.
pub fn keep(at: &Path, pages: WebPages) -> Result<(), String> {
    crate::prefs::save_web_pages(at, pages.key())
}

/// An address the person's own browser may be handed: `http` or `https` (in any case), with no white space or control
/// character anywhere.
pub fn is_web_address(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && url.len() <= 4096
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Open `url` in the person's own browser, once it is a web address ([`is_web_address`]).
pub fn open_yours(url: &str) -> Result<(), String> {
    if !is_web_address(url) {
        return Err("That is not a web address (http or https), so it was not opened.".into());
    }
    launch(url)
}

/// The shell's URL handler. A test build records the address instead, so no test opens a browser.
#[cfg(not(test))]
fn launch(url: &str) -> Result<(), String> {
    super::loopback::open_in_browser(url)
}

#[cfg(test)]
fn launch(url: &str) -> Result<(), String> {
    OPENED.lock().unwrap_or_else(|p| p.into_inner()).push(url.to_string());
    Ok(())
}

/// What a test build would have opened in the person's own browser, in order.
#[cfg(test)]
pub static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether a test build opened `url` in the person's own browser.
#[cfg(test)]
pub fn opened(url: &str) -> bool {
    OPENED.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|u| u == url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_lattices_browser_until_your_browser_is_kept() {
        let dir = crate::sqlite_ro::tests::scratch("web-choice");
        let at = dir.join("nested").join("preferences.json");
        assert_eq!(load(&at), WebPages::Lattice, "nothing kept");
        keep(&at, WebPages::Yours).unwrap();
        assert_eq!(load(&at), WebPages::Yours);
        let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&at).unwrap()).unwrap();
        assert_eq!(kept, serde_json::json!({"web_pages": "yours"}));
        keep(&at, WebPages::Lattice).unwrap();
        assert_eq!(load(&at), WebPages::Lattice);
        for unreadable in ["not json", "{\"web_pages\": \"firefox\"}", "{\"web_pages\": true}", "{}", "[]"] {
            std::fs::write(&at, unreadable).unwrap();
            assert_eq!(load(&at), WebPages::Lattice, "{unreadable}");
        }
    }

    #[test]
    fn your_browser_is_handed_web_addresses_only() {
        for good in ["https://id.api.alelyon.com/account", "http://127.0.0.1:8080/reset", "HTTPS://WWW.ALELYON.COM/"] {
            assert!(is_web_address(good), "{good}");
        }
        for bad in [
            "",
            "file:///C:/Windows/System32/calc.exe",
            "ms-settings:privacy",
            "javascript:alert(1)",
            "alelyon://open",
            "//www.alelyon.com/",
            " https://www.alelyon.com/",
            "https://www.alelyon.com/a b",
            "https://www.alelyon.com/\n",
        ] {
            assert!(!is_web_address(bad), "{bad:?}");
            assert!(open_yours(bad).unwrap_err().contains("not a web address"), "{bad:?}");
            assert!(!opened(bad), "{bad:?} was opened");
        }
        let url = "https://www.alelyon.com/legal/terms/?test=your_browser_is_handed_web_addresses_only";
        open_yours(url).unwrap();
        assert!(opened(url));
    }
}
