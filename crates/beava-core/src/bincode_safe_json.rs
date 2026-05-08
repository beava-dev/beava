//! Format-aware serde adapters for fields that contain bincode-incompatible
//! shapes — `serde_json::Value` and `#[serde(tag = "...")]` enums (e.g.
//! `OpNode`).
//!
//! ## The bug we work around
//!
//! Bincode does not support `Deserializer::deserialize_any`. Two patterns in
//! the registry / payload tree need exactly that on the wire side:
//!
//! 1. `serde_json::Value`'s `Deserialize` calls `deserialize_any` directly —
//!    the wire format (`AggSpec.params`, `OpNode::Fillna.defaults`,
//!    `Value::Json`) carries opaque JSON.
//! 2. `OpNode` is `#[serde(tag = "op")]` (internally-tagged), and serde's
//!    derived deserialize for internally-tagged enums calls `deserialize_any`
//!    to peek the tag before dispatching to a variant.
//!
//! Both work fine on JSON / MsgPack and fail on bincode — which is what
//! `SnapshotBody::decode` uses. Production `recovery.snapshot_decode_failed`
//! at 2026-05-08 04:04:28 was emitted from exactly this path:
//!
//! ```text
//! "error":"bincode: Bincode does not support the
//!  serde::Deserializer::deserialize_any method"
//! ```
//!
//! ## The fix
//!
//! `is_human_readable()` is honest for the formats we use (JSON → true,
//! bincode 1.3 default → false), so the adapter branches:
//!
//! - **Human-readable** path: serialize / deserialize the inner value
//!   natively. The wire format on `/register` and the test fixtures stays
//!   byte-identical.
//! - **Non-human-readable** path: serialize as a JSON-encoded `String` and
//!   parse it back on deserialize. Bincode handles `String` cleanly, and
//!   the inner `serde_json::from_str` uses serde_json's own deserializer
//!   (which supports `deserialize_any` because that's serde_json's whole
//!   data model).
//!
//! This sidesteps both root causes at once — the adapter only needs to be
//! applied at the smallest envelope that contains the bincode-incompatible
//! shape (e.g. the whole `registry: RegistryDescriptorsOnly` field of the
//! snapshot body, since it's the snapshot's only bincode-hostile subtree).
//! Per-field adapters on `AggSpec.params` etc. don't help on their own
//! because bincode bombs out earlier at the `OpNode` tag-decode boundary.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

// ─── single value ─────────────────────────────────────────────────────────────

/// `#[serde(with = "bincode_safe_json::value")]` for a single
/// `serde_json::Value` field.
pub mod value {
    use super::*;

    pub fn serialize<S>(v: &serde_json::Value, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if s.is_human_readable() {
            v.serialize(s)
        } else {
            let json = serde_json::to_string(v).map_err(serde::ser::Error::custom)?;
            json.serialize(s)
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<serde_json::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if d.is_human_readable() {
            serde_json::Value::deserialize(d)
        } else {
            let s = String::deserialize(d)?;
            serde_json::from_str(&s).map_err(serde::de::Error::custom)
        }
    }
}

// ─── BTreeMap<String, serde_json::Value> ──────────────────────────────────────

/// `#[serde(with = "bincode_safe_json::map")]` for a
/// `BTreeMap<String, serde_json::Value>` field (e.g. `OpNode::Fillna.defaults`).
pub mod map {
    use super::*;

    pub fn serialize<S>(m: &BTreeMap<String, serde_json::Value>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if s.is_human_readable() {
            m.serialize(s)
        } else {
            let as_strings: BTreeMap<String, String> = m
                .iter()
                .map(|(k, v)| {
                    serde_json::to_string(v)
                        .map(|s| (k.clone(), s))
                        .map_err(serde::ser::Error::custom)
                })
                .collect::<Result<_, _>>()?;
            as_strings.serialize(s)
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<BTreeMap<String, serde_json::Value>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if d.is_human_readable() {
            BTreeMap::<String, serde_json::Value>::deserialize(d)
        } else {
            let raw = BTreeMap::<String, String>::deserialize(d)?;
            raw.into_iter()
                .map(|(k, s)| {
                    serde_json::from_str(&s)
                        .map(|v| (k, v))
                        .map_err(serde::de::Error::custom)
                })
                .collect()
        }
    }
}

// ─── RegistryDescriptorsOnly (covers the prod snapshot-decode path) ───────────

/// `#[serde(with = "bincode_safe_json::registry")]` for a
/// `RegistryDescriptorsOnly` field. Used on `SnapshotBody.registry` so the
/// snapshot bincode envelope can hold an `OpNode`-bearing derivation — the
/// in-memory enum keeps `#[serde(tag = "op")]` for wire compatibility, while
/// the snapshot path serialises the registry as a single JSON string.
pub mod registry {
    use super::*;
    use crate::snapshot_body::RegistryDescriptorsOnly;

    pub fn serialize<S>(r: &RegistryDescriptorsOnly, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if s.is_human_readable() {
            r.serialize(s)
        } else {
            let json = serde_json::to_string(r).map_err(serde::ser::Error::custom)?;
            json.serialize(s)
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<RegistryDescriptorsOnly, D::Error>
    where
        D: Deserializer<'de>,
    {
        if d.is_human_readable() {
            RegistryDescriptorsOnly::deserialize(d)
        } else {
            let s = String::deserialize(d)?;
            serde_json::from_str(&s).map_err(serde::de::Error::custom)
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        #[serde(with = "value")]
        v: serde_json::Value,
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct MapHolder {
        #[serde(with = "map")]
        m: BTreeMap<String, serde_json::Value>,
    }

    #[test]
    fn value_bincode_roundtrip() {
        let h = Holder {
            v: serde_json::json!({"field": "dwell_ms", "q": 0.5}),
        };
        let bytes = bincode::serialize(&h).unwrap();
        let decoded: Holder = bincode::deserialize(&bytes).unwrap();
        assert_eq!(h, decoded);
    }

    #[test]
    fn value_json_wire_unchanged() {
        // Wire format must remain a native JSON object, not a quoted
        // string. SDKs and `register_pipeline.py --dump` rely on this.
        let h = Holder {
            v: serde_json::json!({"k": 1}),
        };
        let s = serde_json::to_string(&h).unwrap();
        assert_eq!(s, r#"{"v":{"k":1}}"#);
        let decoded: Holder = serde_json::from_str(&s).unwrap();
        assert_eq!(h, decoded);
    }

    #[test]
    fn map_bincode_roundtrip() {
        let mut m: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        m.insert("a".into(), serde_json::json!(1));
        m.insert("b".into(), serde_json::json!("two"));
        let h = MapHolder { m: m.clone() };
        let bytes = bincode::serialize(&h).unwrap();
        let decoded: MapHolder = bincode::deserialize(&bytes).unwrap();
        assert_eq!(h, decoded);
    }

    #[test]
    fn map_json_wire_unchanged() {
        let mut m: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        m.insert("a".into(), serde_json::json!(1));
        let h = MapHolder { m };
        let s = serde_json::to_string(&h).unwrap();
        assert_eq!(s, r#"{"m":{"a":1}}"#);
    }
}
