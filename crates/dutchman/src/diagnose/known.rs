//! Only replay-confirmed signatures belong in the release list. No network lookup.
use serde::{Deserialize, Serialize};

use super::{recipes, signature::Signature};

#[derive(Deserialize)]
struct Catalog {
    schema: String,
    failures: Vec<KnownFailure>,
}

/// A release's advice for one exact signature and recipe class.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnownFailure {
    /// Full SHA-256 signature hash.
    pub hash: String,
    /// Recipe class (feature kind, or `generic`).
    pub class: String,
    /// `fixed_next_version` or `workaround`.
    pub status: String,
    /// Plain-language advice, with no design information.
    pub message: String,
}

fn lookup_in(data: &str, signature: &Signature) -> Option<KnownFailure> {
    let catalog: Catalog = serde_json::from_str(data).ok()?;
    if catalog.schema != "dutchman.known-failures/1" {
        return None;
    }
    catalog.failures.into_iter().find(|failure| {
        failure.hash == signature.hash
            && failure.class == recipes::class(signature)
            && matches!(failure.status.as_str(), "fixed_next_version" | "workaround")
    })
}

/// Exact matching only: a recipe class alone never claims that a problem is known.
pub fn lookup(signature: &Signature) -> Option<KnownFailure> {
    lookup_in(include_str!("../../data/known-failures-v1.json"), signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnose::signature::ErrorCode;
    #[test]
    fn release_list_parses_and_matches_both_hash_and_class() {
        let catalog: Catalog =
            serde_json::from_str(include_str!("../../data/known-failures-v1.json")).unwrap();
        assert_eq!(catalog.schema, "dutchman.known-failures/1");
        let signature = Signature::new(ErrorCode::default(), None);
        let mut data = serde_json::json!({"schema": catalog.schema, "failures": [{
            "hash": signature.hash, "class": "generic", "status": "workaround", "message": "Try a simpler extent."
        }]});
        assert!(lookup_in(&data.to_string(), &signature).is_some());
        data["failures"][0]["class"] = "extrude".into();
        assert!(lookup_in(&data.to_string(), &signature).is_none());
        data["failures"][0]["class"] = "generic".into();
        data["failures"][0]["hash"] = "different".into();
        assert!(lookup_in(&data.to_string(), &signature).is_none());
    }
}
