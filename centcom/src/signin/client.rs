//! The identity service's native routes: the client lives in the `alelyon-identity-client` crate
//! (also published in the public Alelyon-Client repository), with its tests and the contract (its
//! docs/native-sign-in-contract.md). This module keeps `crate::signin::client::*` naming it.

pub use alelyon_identity_client::client::*;
