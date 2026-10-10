//! A provider's sign-in through the system browser, answered on loopback (RFC 8252 with PKCE S256): it lives in the
//! `alelyon-identity-client` crate, with its tests. This module keeps `crate::signin::loopback::*` naming it.

pub use alelyon_identity_client::loopback::*;
