//! A failure's signature: what kind of thing failed and how, and nothing about the design.
//!
//! `dutchman diagnose --narrow` records one for the failure it narrows down, and
//! `dutchman diagnose --same-failure` compares the signature of a small rebuilt file with it.
//! It holds the error (how the conversion ended, its stage, kind and check-finding codes) and,
//! when the failure was narrowed to one, the feature: its kind, its options and the shape of
//! its profile. Every value is a fixed category or a count of loops. It holds no name, path,
//! identity or dimension, so it can be shared.
//!
//! The JSON form (schema [`SCHEMA`]) carries [`Signature::hash`], the SHA-256 of a text form
//! written out field by field here rather than by serde, so a hash stays the same across
//! builds. It changes only with the schema version, and two signatures with the same hash are
//! the same signature. Lists keyed on a signature use the hash.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The schema of a signature, and the first line of the text its hash is taken of.
pub const SCHEMA: &str = "dutchman.signature/1";

/// How long a value of a fixed category can be.
const MAX_TAG: usize = 48;

/// A failure's signature. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// [`SCHEMA`].
    pub schema: String,
    /// The SHA-256 of the signature's text form, in lowercase hex.
    pub hash: String,
    /// How the conversion ended.
    pub error: ErrorCode,
    /// The feature the failure was narrowed to, when it was narrowed to one.
    pub feature: Option<FeatureShape>,
}

/// How a conversion ended.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorCode {
    /// `failed`, `timed_out` or `out_of_memory`.
    pub outcome: String,
    /// The stage it stopped at (`decode`, `map`, `write`...).
    pub stage: Option<String>,
    /// The error's fixed category (`restate`, `encode`, `cadmpeg_checks`, `panic`, `crash`...).
    pub kind: Option<String>,
    /// The check-finding codes, sorted, without their counts.
    pub codes: Vec<String>,
}

/// What a feature is, without its name or its dimensions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureShape {
    /// Its operation (`extrude`, `fillet`, `hole`...).
    pub kind: String,
    /// The choices made in it, by option: `extent` is `two_sided`, `op` is `cut`.
    pub options: BTreeMap<String, String>,
    /// The shape of its profile, for a feature that has one.
    pub profile: Option<ProfileShape>,
}

/// The shape of a profile, in counts and categories.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileShape {
    /// What the profile is made from (`sketch`, `sketch_regions`, `historical_faces`...).
    pub source: String,
    /// How many closed loops it takes.
    pub loops: u32,
    /// How many of them sit inside another; `None` when that cannot be told.
    pub inner_loops: Option<u32>,
    /// The kinds of curve in it (`line`, `arc`, `circle`, `spline`...).
    pub curves: BTreeSet<String>,
}

/// How two signatures compare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// The same failure.
    Same,
    /// The same, but they say too little to call it the same failure: neither has a feature
    /// or a check-finding code, only a broad error category.
    TooBroad,
    /// Different: these fields differ (`outcome`, `stage`, `kind`, `codes`, `feature`,
    /// `options`, `profile`).
    Different(Vec<&'static str>),
}

impl Signature {
    /// The signature of `error`, at `feature` when known. Anything that is not a fixed
    /// category is dropped here, whatever the caller passed.
    pub fn new(error: ErrorCode, feature: Option<FeatureShape>) -> Self {
        let mut signature = Signature {
            schema: SCHEMA.to_owned(),
            hash: String::new(),
            error: clean_error(error),
            feature: feature.map(clean_feature),
        };
        signature.hash = signature.computed_hash();
        signature
    }

    /// A signature read back from its JSON. A signature of another schema version, or one
    /// whose hash does not match its fields, is refused.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let signature: Signature =
            serde_json::from_str(text).map_err(|e| format!("not a failure signature: {e}"))?;
        if signature.schema != SCHEMA {
            return Err(format!(
                "this signature uses {}; this version of Dutchman reads {SCHEMA}",
                signature.schema
            ));
        }
        if signature.computed_hash() != signature.hash {
            return Err("this signature was changed after it was written".to_owned());
        }
        Ok(signature)
    }

    /// The first 12 digits of the hash, to show to people.
    pub fn short(&self) -> &str {
        self.hash.get(..12).unwrap_or(&self.hash)
    }

    /// The text the hash is taken of: one `field=value` line for each field, in a fixed order.
    pub fn canonical(&self) -> String {
        let mut text = format!("{SCHEMA}\n");
        let mut line = |key: &str, value: &str| {
            text.push_str(key);
            text.push('=');
            text.push_str(value);
            text.push('\n');
        };
        line("outcome", &self.error.outcome);
        line("stage", self.error.stage.as_deref().unwrap_or(""));
        line("kind", self.error.kind.as_deref().unwrap_or(""));
        line("codes", &self.error.codes.join(","));
        if let Some(feature) = &self.feature {
            line("feature", &feature.kind);
            for (key, value) in &feature.options {
                line(&format!("option.{key}"), value);
            }
            if let Some(profile) = &feature.profile {
                line("profile.source", &profile.source);
                line("profile.loops", &profile.loops.to_string());
                line(
                    "profile.inner_loops",
                    &profile
                        .inner_loops
                        .map_or(String::new(), |inner| inner.to_string()),
                );
                let curves: Vec<&str> = profile.curves.iter().map(String::as_str).collect();
                line("profile.curves", &curves.join(","));
            }
        }
        text
    }

    fn computed_hash(&self) -> String {
        let digest = Sha256::digest(self.canonical().as_bytes());
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Whether this says enough to tell one failure from another.
    pub fn is_specific(&self) -> bool {
        self.feature.is_some() || !self.error.codes.is_empty()
    }

    /// How `other`, the signature of a rebuilt file, compares with this one.
    pub fn compare(&self, other: &Signature) -> Comparison {
        if self.hash == other.hash {
            return if self.is_specific() {
                Comparison::Same
            } else {
                Comparison::TooBroad
            };
        }
        let mut fields = Vec::new();
        if self.error.outcome != other.error.outcome {
            fields.push("outcome");
        }
        if self.error.stage != other.error.stage {
            fields.push("stage");
        }
        if self.error.kind != other.error.kind {
            fields.push("kind");
        }
        if self.error.codes != other.error.codes {
            fields.push("codes");
        }
        match (&self.feature, &other.feature) {
            (Some(a), Some(b)) => {
                if a.kind != b.kind {
                    fields.push("feature");
                }
                if a.options != b.options {
                    fields.push("options");
                }
                if a.profile != b.profile {
                    fields.push("profile");
                }
            }
            (None, None) => {}
            _ => fields.push("feature"),
        }
        Comparison::Different(fields)
    }

    /// One plain line about the failure.
    pub fn describe(&self) -> String {
        let mut text = match self.error.outcome.as_str() {
            "timed_out" => "ran past the time limit".to_owned(),
            "out_of_memory" => "ran past the memory limit".to_owned(),
            _ => format!(
                "failed with {}",
                self.error.kind.as_deref().unwrap_or("an error")
            ),
        };
        if let Some(stage) = &self.error.stage {
            text.push_str(&format!(" at the {stage} stage"));
        }
        if !self.error.codes.is_empty() {
            text.push_str(&format!(" ({})", self.error.codes.join(", ")));
        }
        if let Some(feature) = &self.feature {
            text.push_str(&format!(" in {}", feature.describe()));
        }
        text
    }
}

impl FeatureShape {
    /// The feature in plain words.
    pub fn describe(&self) -> String {
        let mut text = format!("a {} feature", self.kind.replace('_', " "));
        let options: Vec<String> = self
            .options
            .iter()
            .map(|(key, value)| format!("{} {}", key.replace('_', " "), value.replace('_', " ")))
            .collect();
        if !options.is_empty() {
            text.push_str(&format!(" ({})", options.join(", ")));
        }
        if let Some(profile) = &self.profile {
            text.push_str(&format!(" {}", profile.describe()));
        }
        text
    }
}

impl ProfileShape {
    fn describe(&self) -> String {
        let curves: Vec<&str> = self.curves.iter().map(String::as_str).collect();
        let mut text = format!(
            "from a profile of {} loop{}",
            self.loops,
            if self.loops == 1 { "" } else { "s" }
        );
        if let Some(inner) = self.inner_loops.filter(|inner| *inner > 0) {
            text.push_str(&format!(", {inner} inside another"));
        }
        if !curves.is_empty() {
            text.push_str(&format!(", made of {}", curves.join(", ")));
        }
        text
    }
}

/// Whether `text` is a fixed category: a short lowercase word, as CADIR's and Dutchman's own
/// categories are. Names and paths that happen to look like one never get here: the values
/// come from typed fields only.
fn is_tag(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_TAG
        && text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn clean_error(error: ErrorCode) -> ErrorCode {
    let mut codes: Vec<String> = error.codes.into_iter().filter(|c| is_tag(c)).collect();
    codes.sort();
    codes.dedup();
    ErrorCode {
        outcome: Some(error.outcome)
            .filter(|outcome| is_tag(outcome))
            .unwrap_or_else(|| "failed".to_owned()),
        stage: error.stage.filter(|stage| is_tag(stage)),
        kind: error.kind.filter(|kind| is_tag(kind)),
        codes,
    }
}

fn clean_feature(feature: FeatureShape) -> FeatureShape {
    FeatureShape {
        kind: Some(feature.kind)
            .filter(|kind| is_tag(kind))
            .unwrap_or_else(|| "other".to_owned()),
        options: feature
            .options
            .into_iter()
            .filter(|(key, value)| is_tag(key) && is_tag(value))
            .collect(),
        profile: feature.profile.map(|profile| ProfileShape {
            source: Some(profile.source)
                .filter(|source| is_tag(source))
                .unwrap_or_else(|| "other".to_owned()),
            loops: profile.loops,
            inner_loops: profile.inner_loops,
            curves: profile.curves.into_iter().filter(|c| is_tag(c)).collect(),
        }),
    }
}

/// The options read from a feature's definition: the name shown for each, and where it is.
/// Each of these is a typed CADIR enum, so it can only hold one of CADIR's own words.
const OPTIONS: &[(&str, &str)] = &[
    ("op", "/op"),
    ("direction", "/direction"),
    ("start", "/start"),
    ("extent", "/extent"),
    ("end", "/extent/side/termination"),
    ("end", "/extent/termination"),
    ("first_end", "/extent/first/termination"),
    ("first_end", "/extent/first"),
    ("second_end", "/extent/second/termination"),
    ("second_end", "/extent/second"),
    ("inner_wire_taper", "/inner_wire_taper"),
    ("pattern", "/pattern"),
    ("family", "/family"),
];

/// The operations whose own `kind` is one more CADIR enum.
const TYPED: &[&str] = &["hole", "primitive"];

/// True-or-false options, by the same rule.
const FLAGS: &[(&str, &str)] = &[("solid", "/solid"), ("refine", "/refine")];

/// Options that matter by being there at all: a draft angle's value is a dimension.
const PRESENT: &[(&str, &str)] = &[
    ("draft", "/extent/side/draft"),
    ("draft", "/extent/first/draft"),
    ("draft", "/extent/second/draft"),
];

/// The operation a CADIR feature row stands for, looking through a post-processing wrapper.
fn operation(feature: &Value) -> &Value {
    let definition = feature.get("definition").unwrap_or(&Value::Null);
    match definition.get("definition").and_then(Value::as_str) {
        Some("post_process") => definition.get("operation").unwrap_or(&Value::Null),
        _ => definition,
    }
}

/// A CADIR enum's word, whether it is written as a string or as an object's `kind`.
fn word(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("kind").and_then(Value::as_str))
        .filter(|text| is_tag(text))
}

/// What `feature` (a row of the CADIR `features` arena) is, with `model` the CADIR `model`
/// object it belongs to. Dimensions, names and identities are read only to count loops.
pub fn feature_shape(feature: &Value, model: &Value) -> FeatureShape {
    let definition = operation(feature);
    let kind = definition
        .get("definition")
        .and_then(Value::as_str)
        .filter(|kind| is_tag(kind))
        .unwrap_or("other");
    let mut options = BTreeMap::new();
    // A native feature's fields are the source program's own words, not CADIR's.
    if kind != "native" {
        for (name, pointer) in OPTIONS {
            if let Some(value) = definition.pointer(pointer).and_then(word) {
                options
                    .entry((*name).to_owned())
                    .or_insert_with(|| value.to_owned());
            }
        }
        if TYPED.contains(&kind) {
            if let Some(value) = definition.get("kind").and_then(word) {
                options.insert("type".to_owned(), value.to_owned());
            }
        }
        let wrapper = feature.get("definition").unwrap_or(&Value::Null);
        for (name, pointer) in FLAGS {
            let value = definition
                .pointer(pointer)
                .or_else(|| wrapper.pointer(pointer))
                .and_then(Value::as_bool);
            if let Some(value) = value {
                options.insert((*name).to_owned(), value.to_string());
            }
        }
        for (name, pointer) in PRESENT {
            if definition.pointer(pointer).is_some_and(|v| !v.is_null()) {
                options.insert((*name).to_owned(), "yes".to_owned());
            }
        }
    }
    FeatureShape {
        kind: kind.to_owned(),
        options,
        profile: definition.get("profile").and_then(|p| profile(p, model)),
    }
}

/// A sketch curve's extent, for telling whether one loop sits inside another.
#[derive(Clone, Copy)]
struct Extent {
    min: [f64; 2],
    max: [f64; 2],
}

impl Extent {
    fn around(points: &[[f64; 2]]) -> Option<Self> {
        let first = points.first()?;
        let mut extent = Extent {
            min: *first,
            max: *first,
        };
        for point in points {
            for axis in 0..2 {
                if let (Some(min), Some(max), Some(value)) = (
                    extent.min.get_mut(axis),
                    extent.max.get_mut(axis),
                    point.get(axis),
                ) {
                    *min = min.min(*value);
                    *max = max.max(*value);
                }
            }
        }
        Some(extent)
    }

    fn holds(&self, other: &Extent) -> bool {
        (0..2).all(|axis| {
            let inside = || -> Option<bool> {
                Some(
                    self.min.get(axis)? < other.min.get(axis)?
                        && other.max.get(axis)? < self.max.get(axis)?,
                )
            };
            inside().unwrap_or(false)
        })
    }
}

fn point(value: Option<&Value>) -> Option<[f64; 2]> {
    let value = value?;
    let coordinate =
        |key: &str, at: usize| value.get(key).or_else(|| value.get(at)).and_then(number_of);
    Some([coordinate("x", 0)?, coordinate("y", 1)?])
}

fn number_of(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.get("value").and_then(Value::as_f64))
}

/// The points a sketch curve spans, for the curves whose extent is simple to tell.
fn span(geometry: &Value) -> Option<Vec<[f64; 2]>> {
    match geometry.get("kind").and_then(Value::as_str)? {
        "line" => Some(vec![
            point(geometry.get("start"))?,
            point(geometry.get("end"))?,
        ]),
        "circle" | "arc" => {
            let [x, y] = point(geometry.get("center"))?;
            let radius = number_of(geometry.get("radius")?)?;
            Some(vec![[x - radius, y - radius], [x + radius, y + radius]])
        }
        _ => None,
    }
}

fn profile(profile: &Value, model: &Value) -> Option<ProfileShape> {
    let source = word(profile)?.to_owned();
    let value = profile.get("value").unwrap_or(&Value::Null);
    let mut shape = ProfileShape {
        source,
        ..ProfileShape::default()
    };
    let sketch = value
        .as_str()
        .or_else(|| value.get("sketch").and_then(Value::as_str));
    let Some(sketch) = sketch else {
        return Some(shape);
    };
    let arena = |name: &str| model.get(name).and_then(Value::as_array);
    let row = arena("sketches")?
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(sketch))?;
    let entities: BTreeMap<&str, &Value> = arena("sketch_entities")
        .into_iter()
        .flatten()
        .chain(
            row.get("entities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .filter_map(|entity| Some((entity.get("id")?.as_str()?, entity.get("geometry")?)))
        .collect();
    let empty = Vec::new();
    let loops = row
        .get("profiles")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let chosen: Vec<&Value> = match value.get("profiles").and_then(Value::as_array) {
        Some(indices) => indices
            .iter()
            .filter_map(Value::as_u64)
            .filter_map(|index| loops.get(usize::try_from(index).ok()?))
            .collect(),
        None => loops.iter().collect(),
    };
    shape.loops = u32::try_from(chosen.len()).unwrap_or(u32::MAX);
    let mut extents = Vec::with_capacity(chosen.len());
    for ring in chosen {
        let mut points = Some(Vec::new());
        for used in ring.as_array().unwrap_or(&empty) {
            let geometry = used
                .get("entity")
                .and_then(Value::as_str)
                .and_then(|id| entities.get(id).copied());
            if let Some(kind) = geometry.and_then(|g| g.get("kind")).and_then(word_of) {
                shape.curves.insert(kind.to_owned());
            }
            match (geometry.and_then(span), &mut points) {
                (Some(span), Some(points)) => points.extend(span),
                _ => points = None,
            }
        }
        extents.push(points.and_then(|points| Extent::around(&points)));
    }
    let known: Option<Vec<Extent>> = extents.into_iter().collect();
    shape.inner_loops = known.map(|extents| {
        let inner = extents
            .iter()
            .enumerate()
            .filter(|(at, extent)| {
                extents
                    .iter()
                    .enumerate()
                    .any(|(other, outer)| other != *at && outer.holds(extent))
            })
            .count();
        u32::try_from(inner).unwrap_or(u32::MAX)
    });
    Some(shape)
}

fn word_of(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| is_tag(text))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use serde_json::json;

    use super::*;

    fn error() -> ErrorCode {
        ErrorCode {
            outcome: "failed".into(),
            stage: Some("write".into()),
            kind: Some("restate".into()),
            codes: vec!["parameter_domain".into(), "curve_open".into()],
        }
    }

    fn extrude(name: &str, length: f64) -> (Value, Value) {
        let feature = json!({"id": "f:1", "ordinal": 3, "name": name, "definition": {
            "definition": "extrude",
            "op": "cut",
            "direction": {"kind": "profile_normal"},
            "start": {"kind": "profile_plane"},
            "extent": {"kind": "two_sided",
                "first": {"termination": {"kind": "blind", "length": length}, "draft": 0.1},
                "second": {"termination": {"kind": "through_all"}}},
            "solid": true,
            "profile": {"kind": "sketch", "value": "s:1"},
        }});
        let side = length * 4.0;
        let model = json!({
            "sketches": [{"id": "s:1", "name": name, "profiles": [
                [{"entity": "a"}, {"entity": "b"}, {"entity": "c"}, {"entity": "d"}],
                [{"entity": "hole"}],
            ]}],
            "sketch_entities": [
                {"id": "a", "geometry": {"kind": "line",
                    "start": {"x": 0.0, "y": 0.0}, "end": {"x": side, "y": 0.0}}},
                {"id": "b", "geometry": {"kind": "line",
                    "start": {"x": side, "y": 0.0}, "end": {"x": side, "y": side}}},
                {"id": "c", "geometry": {"kind": "line",
                    "start": {"x": side, "y": side}, "end": {"x": 0.0, "y": side}}},
                {"id": "d", "geometry": {"kind": "line",
                    "start": {"x": 0.0, "y": side}, "end": {"x": 0.0, "y": 0.0}}},
                {"id": "hole", "geometry": {"kind": "circle",
                    "center": {"x": side / 2.0, "y": side / 2.0}, "radius": 1.0}},
            ],
        });
        (feature, model)
    }

    #[test]
    fn a_feature_shape_holds_categories_and_counts_only() {
        let (feature, model) = extrude("SecretBracket", 12.5);
        let shape = feature_shape(&feature, &model);
        assert_eq!(shape.kind, "extrude");
        assert_eq!(shape.options["extent"], "two_sided");
        assert_eq!(shape.options["op"], "cut");
        assert_eq!(shape.options["first_end"], "blind");
        assert_eq!(shape.options["second_end"], "through_all");
        assert_eq!(shape.options["draft"], "yes");
        assert_eq!(shape.options["solid"], "true");
        let profile = shape.profile.clone().unwrap();
        assert_eq!(profile.source, "sketch");
        assert_eq!(profile.loops, 2);
        assert_eq!(profile.inner_loops, Some(1));
        assert_eq!(
            profile
                .curves
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["circle", "line"]
        );
        let text = serde_json::to_string(&Signature::new(error(), Some(shape))).unwrap();
        for private in ["Secret", "12.5", "s:1", "f:1", "\"hole\""] {
            assert!(!text.contains(private), "{private} in {text}");
        }
    }

    #[test]
    fn the_same_feature_at_other_sizes_with_other_names_has_the_same_hash() {
        let (a, model_a) = extrude("Private one", 12.5);
        let (b, model_b) = extrude("Rebuilt", 3.0);
        let a = Signature::new(error(), Some(feature_shape(&a, &model_a)));
        let b = Signature::new(error(), Some(feature_shape(&b, &model_b)));
        assert_eq!(a.hash, b.hash);
        assert_eq!(a.compare(&b), Comparison::Same);
    }

    #[test]
    fn a_native_features_own_words_stay_out() {
        let feature = json!({"definition": {"definition": "native", "kind": "mysecretfeature",
            "extent": {"kind": "secretword"}}});
        let shape = feature_shape(&feature, &json!({}));
        assert_eq!(shape.kind, "native");
        assert!(shape.options.is_empty());
    }

    #[test]
    fn what_is_not_a_category_is_dropped() {
        let signature = Signature::new(
            ErrorCode {
                outcome: "failed".into(),
                stage: Some("/home/someone/file.f3d".into()),
                kind: Some("Secret Part".into()),
                codes: vec!["fine_code".into(), "Not A Code".into()],
            },
            Some(FeatureShape {
                kind: "Secret Feature".into(),
                options: BTreeMap::from([("op".into(), "Secret".into())]),
                profile: None,
            }),
        );
        assert_eq!(signature.error.stage, None);
        assert_eq!(signature.error.kind, None);
        assert_eq!(signature.error.codes, ["fine_code"]);
        let feature = signature.feature.unwrap();
        assert_eq!(feature.kind, "other");
        assert!(feature.options.is_empty());
    }

    /// The hash is a contract: lists of known failures key on it. If this test fails the
    /// schema version has to change with it.
    #[test]
    fn the_hash_of_a_known_signature_does_not_change() {
        let signature = Signature::new(
            ErrorCode {
                outcome: "failed".into(),
                stage: Some("write".into()),
                kind: Some("restate".into()),
                codes: vec!["b".into(), "a".into()],
            },
            Some(FeatureShape {
                kind: "extrude".into(),
                options: BTreeMap::from([("extent".into(), "two_sided".into())]),
                profile: Some(ProfileShape {
                    source: "sketch".into(),
                    loops: 4,
                    inner_loops: Some(3),
                    curves: BTreeSet::from(["line".into()]),
                }),
            }),
        );
        assert_eq!(
            signature.canonical(),
            "dutchman.signature/1\noutcome=failed\nstage=write\nkind=restate\ncodes=a,b\n\
             feature=extrude\noption.extent=two_sided\nprofile.source=sketch\nprofile.loops=4\n\
             profile.inner_loops=3\nprofile.curves=line\n"
        );
        let digest = Sha256::digest(signature.canonical().as_bytes());
        let expected: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(signature.hash, expected);
        assert_eq!(signature.hash.len(), 64);
        assert_eq!(signature.short().len(), 12);
    }

    #[test]
    fn signatures_read_back_and_refuse_changes_and_other_versions() {
        let signature = Signature::new(error(), None);
        let text = serde_json::to_string(&signature).unwrap();
        assert_eq!(Signature::from_json(&text).unwrap(), signature);
        let edited = text.replace("restate", "encode");
        assert!(Signature::from_json(&edited)
            .unwrap_err()
            .contains("changed"));
        let newer = text.replace(SCHEMA, "dutchman.signature/2");
        assert!(Signature::from_json(&newer)
            .unwrap_err()
            .contains("dutchman.signature/2"));
        assert!(Signature::from_json("{}").is_err());
    }

    #[test]
    fn a_comparison_names_what_differs_and_will_not_claim_a_broad_match() {
        let (feature, model) = extrude("A", 1.0);
        let shape = feature_shape(&feature, &model);
        let a = Signature::new(error(), Some(shape.clone()));
        let mut other = shape;
        other.kind = "revolve".into();
        let b = Signature::new(error(), Some(other));
        assert_eq!(a.compare(&b), Comparison::Different(vec!["feature"]));
        let none = Signature::new(error(), None);
        assert_eq!(a.compare(&none), Comparison::Different(vec!["feature"]));
        let broad = Signature::new(
            ErrorCode {
                outcome: "failed".into(),
                stage: Some("write".into()),
                kind: Some("encode".into()),
                codes: Vec::new(),
            },
            None,
        );
        assert_eq!(broad.compare(&broad.clone()), Comparison::TooBroad);
    }
}
