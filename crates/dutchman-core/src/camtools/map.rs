//! One Fusion tool to one CamCraft tool definition: the mapping table in `super`'s docs, and
//! CamCraft's own rules for what a tool may be (a tool it would refuse is never written,
//! because CamCraft refuses a whole library for one such tool).

use std::cell::RefCell;
use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

/// CamCraft's tool kinds, by the word its file writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    EndMill,
    BallNose,
    BullNose,
    Drill,
    Tap,
    Bore,
    Turning,
    Groove,
    Thread,
    Form,
    Taper,
    Probe,
    Torch,
    Waterjet,
    ThreadMill,
    Keyseat,
    Dovetail,
    Lollipop,
}

impl Kind {
    pub(super) fn word(self) -> &'static str {
        match self {
            Kind::EndMill => "end_mill",
            Kind::BallNose => "ball_nose",
            Kind::BullNose => "bull_nose",
            Kind::Drill => "drill",
            Kind::Tap => "tap",
            Kind::Bore => "bore",
            Kind::Turning => "turning",
            Kind::Groove => "groove",
            Kind::Thread => "thread",
            Kind::Form => "form",
            Kind::Taper => "taper",
            Kind::Probe => "probe",
            Kind::Torch => "torch",
            Kind::Waterjet => "waterjet",
            Kind::ThreadMill => "thread_mill",
            Kind::Keyseat => "keyseat",
            Kind::Dovetail => "dovetail",
            Kind::Lollipop => "lollipop",
        }
    }

    fn undercut(self) -> bool {
        matches!(self, Kind::Keyseat | Kind::Dovetail | Kind::Lollipop)
    }

    fn lathe_or_bore(self) -> bool {
        self.lathe() || self == Kind::Bore
    }

    fn lathe(self) -> bool {
        matches!(
            self,
            Kind::Turning | Kind::Groove | Kind::Thread | Kind::Form
        )
    }

    /// A kind that turns no spindle: its spindle speed is 0 by CamCraft's rule.
    fn spindleless(self) -> bool {
        matches!(self, Kind::Torch | Kind::Waterjet)
    }
}

/// One row of CamCraft's cutting data: the numbers for one workpiece material.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub material: &'static str,
    pub rpm: f64,
    pub feed: f64,
    pub stepdown: Option<f64>,
    pub stepover: Option<f64>,
}

/// A tool as CamCraft's file states it. Lengths are millimetres, angles degrees.
#[derive(Clone, Debug)]
pub(super) struct Def {
    pub kind: Kind,
    pub diameter: f64,
    pub corner_r: f64,
    pub tip_angle: f64,
    pub pitch: f64,
    pub left_hand: bool,
    pub nose_radius: f64,
    pub width: f64,
    pub flute_len: f64,
    pub shank_d: f64,
    pub shank_len: f64,
    pub holder_d: f64,
    pub holder_len: f64,
    pub material_group: String,
    pub rpm: f64,
    pub feed: f64,
    pub tool_material: Option<&'static str>,
    pub flutes: Option<u32>,
    pub center_cutting: bool,
    pub part_number: Option<String>,
    pub vendor: Option<String>,
    pub notes: Option<String>,
    pub rows: Vec<Row>,
    /// Surface speed (m/min) and feed per revolution (mm) of a constant-surface-speed tool.
    pub turning: Option<(f64, f64)>,
}

impl Def {
    /// The `definition` object CamCraft's reader takes.
    pub(super) fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("kind".into(), json!(self.kind.word()));
        o.insert("diameter".into(), json!(self.diameter));
        o.insert("corner_r".into(), json!(self.corner_r));
        o.insert("tip_angle".into(), json!(self.tip_angle));
        o.insert("pitch".into(), json!(self.pitch));
        if self.left_hand {
            o.insert("thread_hand".into(), json!("left"));
        }
        o.insert("nose_radius".into(), json!(self.nose_radius));
        o.insert("width".into(), json!(self.width));
        o.insert("flute_len".into(), json!(self.flute_len));
        o.insert("shank_d".into(), json!(self.shank_d));
        o.insert("shank_len".into(), json!(self.shank_len));
        o.insert("holder_d".into(), json!(self.holder_d));
        o.insert("holder_len".into(), json!(self.holder_len));
        o.insert("material_group".into(), json!(self.material_group));
        o.insert("rpm".into(), json!(self.rpm));
        o.insert("feed".into(), json!(self.feed));
        if let Some(m) = self.tool_material {
            o.insert("tool_material".into(), json!(m));
        }
        if let Some(z) = self.flutes {
            o.insert("flutes".into(), json!(z));
        }
        if !self.center_cutting {
            o.insert("center_cutting".into(), json!(false));
        }
        for (key, value) in [
            ("part_number", &self.part_number),
            ("vendor", &self.vendor),
            ("notes", &self.notes),
        ] {
            if let Some(v) = value {
                o.insert(key.into(), json!(v));
            }
        }
        if let Some((vc, feed_per_rev)) = self.turning {
            o.insert(
                "turning".into(),
                json!({"vc": vc, "feed_per_rev": feed_per_rev}),
            );
        }
        if !self.rows.is_empty() {
            let rows: Vec<Value> = self
                .rows
                .iter()
                .map(|r| {
                    let mut row = Map::new();
                    row.insert("material".into(), json!(r.material));
                    row.insert("rpm".into(), json!(r.rpm));
                    row.insert("feed".into(), json!(r.feed));
                    if let Some(s) = r.stepdown {
                        row.insert("stepdown".into(), json!(s));
                    }
                    if let Some(s) = r.stepover {
                        row.insert("stepover".into(), json!(s));
                    }
                    Value::Object(row)
                })
                .collect();
            o.insert("cutting_data".into(), Value::Array(rows));
        }
        Value::Object(o)
    }

    /// The `.camtools` version this tool needs: 2 for a left-hand tap, 3 for a stepdown or
    /// stepover in a cutting row.
    pub(super) fn version(&self) -> u64 {
        let mut v = 1;
        if self.kind == Kind::Tap && self.left_hand {
            v = 2;
        }
        if self
            .rows
            .iter()
            .any(|r| r.stepdown.is_some() || r.stepover.is_some())
        {
            v = 3;
        }
        v
    }

    /// The first CamCraft rule this tool breaks, in words, or `None` when CamCraft takes it.
    /// CamCraft commit c5bb09e, `tool_sanity.rs` and the files it calls.
    pub(super) fn refusal(&self) -> Option<String> {
        let (d, a, w, r) = (self.diameter, self.tip_angle, self.width, self.nose_radius);
        let numbers = [
            self.diameter,
            self.corner_r,
            self.tip_angle,
            self.pitch,
            self.nose_radius,
            self.width,
            self.flute_len,
            self.shank_d,
            self.shank_len,
            self.holder_d,
            self.holder_len,
            self.rpm,
            self.feed,
        ];
        if numbers.iter().any(|v| !v.is_finite()) {
            return Some("a size is not a finite number".into());
        }
        if !(d > 0.0 && d <= 500.0) {
            return Some(format!("diameter {d} mm is outside 0..500 mm"));
        }
        if self.corner_r < 0.0 || self.corner_r > d * 0.5 {
            return Some(format!(
                "corner radius {} mm is not between 0 and half the {d} mm diameter",
                self.corner_r
            ));
        }
        if a >= 180.0 {
            return Some(format!("tip angle {a} degrees must be under 180"));
        }
        for (name, v) in [
            ("pitch", self.pitch),
            ("nose radius", r),
            ("width", w),
            ("shank diameter", self.shank_d),
            ("shank length", self.shank_len),
            ("holder diameter", self.holder_d),
            ("holder length", self.holder_len),
            ("tip angle", a),
        ] {
            if v < 0.0 {
                return Some(format!("{name} {v} is negative"));
            }
        }
        if self.flute_len <= 0.0 {
            return Some(format!("flute length {} mm must be over 0", self.flute_len));
        }
        if self.kind.spindleless() {
            if self.rpm != 0.0 {
                return Some("a tool without a spindle must state 0 rpm".into());
            }
        } else if self.rpm <= 0.0 {
            return Some(format!("spindle speed {} rpm must be over 0", self.rpm));
        }
        if self.feed <= 0.0 {
            return Some(format!("feed {} mm/min must be over 0", self.feed));
        }
        if self.flutes.is_some_and(|z| !(1..=32).contains(&z)) {
            return Some("flute count is outside 1..32".into());
        }
        for (what, v) in [("part number", &self.part_number), ("vendor", &self.vendor)] {
            if v.as_ref().is_some_and(|s| s.chars().count() > 80) {
                return Some(format!("{what} is longer than 80 characters"));
            }
        }
        if self.notes.as_ref().is_some_and(|s| s.chars().count() > 500) {
            return Some("notes are longer than 500 characters".into());
        }
        if let Some((vc, fpr)) = self.turning {
            if !(vc > 0.0 && vc <= 2000.0 && fpr > 0.0 && fpr <= 3.0) {
                return Some(format!(
                    "turning surface speed {vc} m/min or feed {fpr} mm/rev is outside CamCraft's limits"
                ));
            }
        }
        if let Some(why) = self.row_refusal() {
            return Some(why);
        }
        self.kind_refusal(d, a, w, r)
    }

    fn row_refusal(&self) -> Option<String> {
        for (i, row) in self.rows.iter().enumerate() {
            if self.rows.iter().take(i).any(|p| p.material == row.material) {
                return Some(format!("cutting data names {} twice", row.material));
            }
            if !(row.feed.is_finite() && row.feed > 0.0 && row.rpm.is_finite() && row.rpm > 0.0) {
                return Some(format!(
                    "cutting data for {}: rpm and feed must be over 0",
                    row.material
                ));
            }
            if [row.stepdown, row.stepover]
                .into_iter()
                .flatten()
                .any(|v| !(v.is_finite() && v > 0.0))
            {
                return Some(format!(
                    "cutting data for {}: stepdown and stepover must be over 0",
                    row.material
                ));
            }
        }
        None
    }

    fn kind_refusal(&self, d: f64, a: f64, w: f64, r: f64) -> Option<String> {
        match self.kind {
            Kind::Drill if a <= 60.0 => Some(format!("a drill's point angle {a} degrees must be over 60")),
            Kind::Tap if self.pitch <= 0.0 => Some("a tap's thread pitch must be over 0".into()),
            Kind::Turning if !(r > 0.0 && r <= d * 0.5) => {
                Some(format!("a turning insert's nose radius {r} mm must be over 0 and at most half the {d} mm size"))
            }
            Kind::Groove | Kind::Thread | Kind::Form if !(w > 0.0 && w < d) => {
                Some(format!("the insert's width {w} mm must be over 0 and under its {d} mm size"))
            }
            Kind::Thread if !(a > 0.0 && a < 180.0) => {
                Some(format!("a threading insert's included angle {a} degrees must be over 0 and under 180"))
            }
            Kind::Form if r <= 0.0 => Some("a form tool's crown radius must be over 0".into()),
            Kind::Form if w > 2.0 * r => Some("a form tool's width is more than twice its crown radius".into()),
            Kind::Taper if a <= 0.0 => Some(format!("a chamfer mill's cone angle {a} degrees must be over 0")),
            Kind::Probe if (self.corner_r - d * 0.5).abs() >= 1e-9 || self.flute_len < d => {
                Some("a probe's stylus must be at least as long as its ball is wide".into())
            }
            Kind::Torch if self.corner_r != 0.0 => Some("a plasma torch carries no corner radius".into()),
            Kind::Waterjet if self.corner_r != 0.0 => Some("a waterjet carries no corner radius".into()),
            Kind::ThreadMill => self.thread_mill_refusal(a),
            k if k.undercut() => self.undercut_refusal(),
            _ => None,
        }
    }

    fn thread_mill_refusal(&self, a: f64) -> Option<String> {
        let (d, p) = (self.diameter, self.pitch);
        if !(a > 0.0 && a < 180.0) {
            return Some(format!(
                "a thread mill's form angle {a} degrees must be over 0 and under 180"
            ));
        }
        if p <= 0.0 {
            return Some("a thread mill's pitch must be over 0".into());
        }
        if d * 0.5 <= 0.6134 * p {
            return Some(format!(
                "a thread mill of {d} mm leaves no core under the teeth of a {p} mm pitch"
            ));
        }
        if self.flute_len < p {
            return Some(format!(
                "a thread mill's cutting length {} mm is shorter than one {p} mm pitch",
                self.flute_len
            ));
        }
        None
    }

    fn undercut_refusal(&self) -> Option<String> {
        let (d, sd, fl) = (self.diameter, self.shank_d, self.flute_len);
        let (r, sr) = (d * 0.5, sd * 0.5);
        if !(sd > 0.0 && sd < d) {
            return Some(format!(
                "the neck diameter {sd} mm must be over 0 and under the {d} mm head"
            ));
        }
        if self.shank_len <= 0.0 {
            return Some("the neck length must be over 0".into());
        }
        match self.kind {
            Kind::Dovetail => {
                let a = self.tip_angle;
                if !(a > 0.0 && a < 90.0) {
                    return Some(format!(
                        "a dovetail's flank angle {a} degrees must be over 0 and under 90"
                    ));
                }
                if r - fl / a.to_radians().tan() < sr {
                    return Some(format!(
                        "a dovetail's {a} degree flank narrows its head under its neck"
                    ));
                }
                None
            }
            Kind::Lollipop => {
                let meet = r + (r * r - sr * sr).max(0.0).sqrt();
                if !(fl >= r && fl <= meet + 1e-9) {
                    return Some(format!(
                        "a lollipop's flute length {fl} mm must be from {r} mm to {meet:.3} mm"
                    ));
                }
                None
            }
            _ => None,
        }
    }
}

/// What mapping one tool gave: the CamCraft tool, or why there is none, plus what it cost.
pub(super) struct Mapped {
    pub name: String,
    pub result: Result<Def, String>,
    /// Approximations (`Degraded`): the tool is written, but not exactly.
    pub approximations: Vec<String>,
    /// Facts that did not need a decision (`Info`): a merged kind, a derived value.
    pub notes: Vec<String>,
    /// Field paths with data that CamCraft has no place for.
    pub not_carried: Vec<String>,
}

/// Reads fields of one Fusion tool and remembers which it read.
struct Src<'a> {
    tool: &'a Value,
    to_mm: f64,
    read: RefCell<BTreeSet<String>>,
}

impl<'a> Src<'a> {
    fn mark(&self, path: String) {
        self.read.borrow_mut().insert(path);
    }

    fn section(&self, section: &str) -> Option<&'a Map<String, Value>> {
        self.tool.get(section).and_then(Value::as_object)
    }

    /// A number in `section` under any of `keys`, as written.
    fn raw(&self, section: &str, keys: &[&str]) -> Option<f64> {
        let map = self.section(section)?;
        let mut found = None;
        for key in keys {
            if let Some(value) = map.get(*key) {
                self.mark(format!("{section}.{key}"));
                if found.is_none() {
                    found = number(value);
                }
            }
        }
        found
    }

    /// A length in `section`, in millimetres.
    fn len(&self, section: &str, keys: &[&str]) -> Option<f64> {
        self.raw(section, keys).map(|v| round6(v * self.to_mm))
    }

    fn text(&self, key: &str) -> Option<String> {
        let v = self.tool.get(key)?;
        self.mark(key.to_owned());
        clean_text(v.as_str()?)
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite())
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn clean_text(s: &str) -> Option<String> {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t = t.trim().to_owned();
    (!t.is_empty()).then_some(t)
}

/// The Fusion type words CamCraft has a kind for, and how.
enum Plan {
    Kind(Kind),
    /// Mapped to this kind, but not the same tool: says what changes.
    Approx(Kind, &'static str),
    Skip(&'static str),
}

fn plan(type_word: &str) -> Plan {
    match type_word {
        "flat end mill" => Plan::Kind(Kind::EndMill),
        "ball end mill" => Plan::Kind(Kind::BallNose),
        "bull nose end mill" => Plan::Kind(Kind::BullNose),
        "chamfer mill" => Plan::Kind(Kind::Taper),
        "dovetail mill" => Plan::Kind(Kind::Dovetail),
        "lollipop mill" => Plan::Kind(Kind::Lollipop),
        "thread mill" => Plan::Kind(Kind::ThreadMill),
        "drill" => Plan::Kind(Kind::Drill),
        "tap right hand" | "tap left hand" => Plan::Kind(Kind::Tap),
        "boring head" | "turning boring" => Plan::Kind(Kind::Bore),
        "probe" => Plan::Kind(Kind::Probe),
        "plasma cutter" => Plan::Kind(Kind::Torch),
        "water jet cutter" => Plan::Kind(Kind::Waterjet),
        "turning general" => Plan::Kind(Kind::Turning),
        "turning grooving" => Plan::Kind(Kind::Groove),
        "turning threading" => Plan::Kind(Kind::Thread),
        "turning form" => Plan::Kind(Kind::Form),
        "face mill" => Plan::Approx(
            Kind::EndMill,
            "CamCraft has no face mill: written as a flat end mill that does not cut at its centre",
        ),
        "slot mill" => Plan::Approx(
            Kind::Keyseat,
            "CamCraft has no slot mill: written as a keyseat cutter (a disc on a neck)",
        ),
        "center drill" | "spot drill" => Plan::Approx(
            Kind::Drill,
            "CamCraft has one drill kind: a center or spot drill is written as a drill",
        ),
        "counter sink" => Plan::Approx(
            Kind::Taper,
            "CamCraft has no countersink: written as a chamfer mill with the countersink's angle",
        ),
        "tapered mill" => Plan::Kind(Kind::Taper),
        "radius mill" => Plan::Skip("CamCraft has no radius (corner-rounding) mill"),
        // A milling form tool is carried only as the multi-form thread mill its profile draws.
        "form mill" => Plan::Kind(Kind::ThreadMill),
        "counter bore" => Plan::Skip("CamCraft has no counterbore tool"),
        "reamer" => Plan::Skip("CamCraft has no reamer"),
        "laser cutter" => Plan::Skip("CamCraft has no laser tool"),
        "marking" => Plan::Skip("CamCraft has no marking tool"),
        "turning custom" => Plan::Skip("a custom turning insert has no CamCraft equivalent"),
        _ => Plan::Skip("this tool type is not one CamCraft has"),
    }
}

/// Fusion's workpiece material categories, and the CamCraft material group each means.
fn material_word(category: &str) -> Option<&'static str> {
    match category.trim().to_ascii_lowercase().as_str() {
        "aluminum" | "aluminium" => Some("aluminium_6061"),
        "steel" => Some("mild_steel"),
        "stainless steel" | "stainless" => Some("stainless_304"),
        "cast iron" => Some("grey_cast_iron"),
        "brass" => Some("brass"),
        "plastic" | "plastics" => Some("plastics_pom"),
        _ => None,
    }
}

/// Keys that carry no data a CamCraft tool could hold, and are never reported.
const QUIET_TOP: [&str; 8] = [
    "guid",
    "reference_guid",
    "expressions",
    "version",
    "last_modified",
    "GRADE",
    "unit",
    "type",
];
const QUIET_SECTION: [&str; 5] = ["guid", "expressions", "version", "name", "description"];

fn meaningful(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !matches!(
            s.trim().to_ascii_lowercase().as_str(),
            "" | "disabled" | "off" | "none"
        ),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Bool(true) => true,
    }
}

/// Maps one Fusion tool.
pub(super) fn map_tool(tool: &Value, index: usize) -> Mapped {
    let type_word = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let unit = tool
        .get("unit")
        .and_then(Value::as_str)
        .unwrap_or("millimeters")
        .trim()
        .to_ascii_lowercase();
    let post = tool.get("post-process").and_then(Value::as_object);
    let number_of = post
        .and_then(|p| p.get("number"))
        .and_then(number)
        .map(|n| n.round() as i64);
    let mut out = Mapped {
        name: String::new(),
        result: Err(String::new()),
        approximations: Vec::new(),
        notes: Vec::new(),
        not_carried: Vec::new(),
    };
    let to_mm = match unit.as_str() {
        "millimeters" | "millimetres" | "mm" => 1.0,
        "inches" | "inch" | "in" => 25.4,
        other => {
            out.name = fallback_name(tool, &type_word, index);
            out.result = Err(format!(
                "its unit \"{other}\" is neither millimeters nor inches"
            ));
            return out;
        }
    };
    let src = Src {
        tool,
        to_mm,
        read: RefCell::new(BTreeSet::new()),
    };
    let description = src.text("description");
    out.name = description
        .clone()
        .unwrap_or_else(|| fallback_name(tool, &type_word, index));
    src.mark("type".into());
    src.mark("unit".into());
    let kind = match plan(&type_word) {
        Plan::Kind(k) => k,
        Plan::Approx(k, why) => {
            out.approximations.push(why.to_owned());
            k
        }
        Plan::Skip(why) => {
            out.result = Err(if type_word.is_empty() {
                "it names no type".to_owned()
            } else {
                format!("type \"{type_word}\": {why}")
            });
            return out;
        }
    };
    out.result = build(
        &src,
        kind,
        &type_word,
        number_of,
        &mut out.approximations,
        &mut out.notes,
    );
    out.not_carried = unread(&src);
    out
}

fn fallback_name(tool: &Value, type_word: &str, index: usize) -> String {
    let unit = match tool
        .get("unit")
        .and_then(Value::as_str)
        .map(|u| u.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("inches" | "inch" | "in") => Unit::Inch,
        Some("millimeters" | "millimetres" | "mm") => Unit::Mm,
        _ => Unit::Unstated,
    };
    let dia = tool
        .get("geometry")
        .and_then(|g| g.get("DC"))
        .and_then(number)
        .map(|d| format!(" {}", size_label(d, unit)))
        .unwrap_or_default();
    let base = if type_word.is_empty() {
        "tool"
    } else {
        type_word
    };
    format!("{base}{dia} (library entry {})", index.saturating_add(1))
}

/// The unit a tool states its sizes in, for naming it.
#[derive(Clone, Copy)]
enum Unit {
    Inch,
    Mm,
    /// No unit, or one that is neither: the size is printed bare.
    Unstated,
}

/// A size for a generated tool name, without float noise: an inch size as a common fraction
/// (up to 64ths) where it is one, else at most 4 decimals; a millimetre size at most 3 decimals;
/// a size in no known unit at most 4 decimals and no unit word. Trailing zeros are trimmed.
fn size_label(value: f64, unit: Unit) -> String {
    match unit {
        Unit::Mm => return format!("{} mm", trimmed(value, 3)),
        Unit::Unstated => return trimmed(value, 4),
        Unit::Inch => {}
    }
    let sixty_fourths = value * 64.0;
    let nearest = sixty_fourths.round();
    if nearest >= 1.0 && (sixty_fourths - nearest).abs() < 1e-6 * nearest.max(1.0) {
        let mut n = nearest as u64;
        let mut d = 64_u64;
        // Stop at a denominator of 1: two inches is 128/64, whose numerator stays even.
        while d > 1 && n.is_multiple_of(2) {
            n /= 2;
            d /= 2;
        }
        let (whole, rest) = (n / d, n % d);
        return match (whole, rest) {
            (w, 0) => format!("{w}\""),
            (0, r) => format!("{r}/{d}\""),
            (w, r) => format!("{w}-{r}/{d}\""),
        };
    }
    format!("{}\"", trimmed(value, 4))
}

/// `value` with at most `places` decimals, trailing zeros (and a bare point) trimmed.
fn trimmed(value: f64, places: usize) -> String {
    let text = trim_zeros(format!("{value:.places$}"));
    if text != "0" || value == 0.0 || !value.is_finite() {
        return text;
    }
    // A size that rounds to nothing keeps two significant digits instead of reading as 0.
    let digits = (-value.abs().log10()).ceil().clamp(0.0, 12.0) as usize + 1;
    trim_zeros(format!("{value:.digits$}"))
}

/// `text` without trailing zeros after a decimal point, and without a bare point or a `-0`.
fn trim_zeros(text: String) -> String {
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    };
    if text == "-0" {
        "0".to_owned()
    } else {
        text
    }
}

#[allow(clippy::too_many_lines)]
fn build(
    src: &Src<'_>,
    kind: Kind,
    type_word: &str,
    number: Option<i64>,
    approximations: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<Def, String> {
    let lathe = kind.lathe_or_bore();
    let tapered = type_word == "tapered mill";
    let form = if type_word == "form mill" {
        Some(read_form(src)?)
    } else {
        None
    };
    let tapered_tip = if tapered {
        Some(tapered_tip(src)?)
    } else {
        None
    };
    let diameter = if let Some(f) = &form {
        let _ = src.len("geometry", &["DC", "diameter"]);
        f.crest_d
    } else if let Some(d) = tapered_tip {
        d
    } else {
        src.len(
            "geometry",
            if lathe {
                &["DC", "diameter", "INSD", "IC"][..]
            } else {
                &["DC", "diameter"][..]
            },
        )
        .filter(|d| *d > 0.0)
        .ok_or("it states no diameter")?
    };
    let flute = src.len("geometry", &["LCF", "flute-length", "LE"]);
    let body = src.len("geometry", &["LB", "body-length"]);
    let overall = src.len("geometry", &["OAL", "overall-length"]);
    let shaft = src.len("geometry", &["SFDM", "shaft-diameter"]);
    let corner = src.len("geometry", &["RE", "corner-radius"]);
    let taper = src.raw("geometry", &["TA", "taper-angle"]);
    let point = src.raw("geometry", &["SIG", "tip-angle"]);
    let pitch = src.len("geometry", &["TP", "thread-pitch"]);
    let profile = src.raw("geometry", &["thread-profile-angle"]);
    let width = src
        .len(
            "geometry",
            &["W", "width", "tool_grooveWidth", "tool_insertWidth"],
        )
        .filter(|w| *w > 0.0);
    let flutes = src
        .raw("geometry", &["NOF", "number-of-flutes"])
        .map(|z| z.round() as i64);

    let mut flute_len = flute.filter(|v| *v > 0.0);
    if flute_len.is_none() {
        flute_len = body.or(overall).filter(|v| *v > 0.0);
        if flute_len.is_some() {
            notes.push("no flute length: the body length was used".into());
        }
    }
    if flute_len.is_none() && lathe {
        flute_len = Some(diameter);
        notes.push("no cutting length: the insert size was used".into());
    }
    let mut flute_len = flute_len.ok_or("it states no flute (cutting) length")?;
    if let Some(f) = &form {
        flute_len = round6(f.rows as f64 * f.pitch);
        notes.push(format!(
            "cutting length {flute_len} mm is the {} tooth rows the profile draws",
            f.rows
        ));
    }

    let reach = body.or(overall).unwrap_or(0.0);
    let shank_len = if reach > flute_len {
        round6(reach - flute_len)
    } else {
        0.0
    };
    let shoulder = if kind.undercut() {
        src.len("geometry", &["shoulder-diameter"])
            .filter(|v| *v > 0.0)
    } else {
        None
    };
    let mut shank_d = shaft.filter(|v| *v > 0.0).unwrap_or(diameter);
    if let Some(f) = &form {
        shank_d = f.shank_d;
    }
    if kind.undercut() {
        // Fusion's shoulder is the narrowed part between the head and the shank: CamCraft's neck.
        if let Some(sd) = shoulder {
            shank_d = sd;
        }
    }
    if kind.undercut() && shaft.is_none() && shoulder.is_none() {
        // A neck CamCraft can judge needs its own diameter; the library gave none.
        shank_d = 0.0;
    }
    if shank_len == 0.0 && !kind.undercut() {
        notes.push("no body or overall length beyond the flutes: shank length 0".into());
    }

    let (holder_d, holder_len) = holder_of(src, notes);

    let mut def = Def {
        kind,
        diameter,
        corner_r: 0.0,
        tip_angle: 0.0,
        pitch: 0.0,
        left_hand: false,
        nose_radius: 0.0,
        width: 0.0,
        flute_len,
        shank_d,
        shank_len,
        holder_d,
        holder_len,
        material_group: String::new(),
        rpm: 0.0,
        feed: 0.0,
        tool_material: None,
        flutes: flutes
            .and_then(|z| u32::try_from(z).ok())
            .filter(|z| (1..=32).contains(z)),
        center_cutting: true,
        part_number: src.text("product-id").map(|s| trim_to(&s, 80)),
        vendor: src.text("vendor").map(|s| trim_to(&s, 80)),
        notes: None,
        rows: Vec::new(),
        turning: None,
    };

    match kind {
        Kind::EndMill | Kind::BullNose => def.corner_r = corner.unwrap_or(0.0),
        Kind::BallNose => def.corner_r = round6(diameter / 2.0),
        Kind::Taper => {
            def.tip_angle = if tapered {
                taper.map(|t| 2.0 * t).ok_or("it states no taper angle")?
            } else if type_word == "counter sink" {
                point
                    .or_else(|| taper.map(|t| 2.0 * t))
                    .ok_or("it states no countersink angle")?
            } else {
                taper.map(|t| 2.0 * t).ok_or("it states no taper angle")?
            };
        }
        Kind::Dovetail => {
            def.tip_angle = taper.map(|t| 90.0 - t).ok_or("it states no taper angle")?;
        }
        Kind::Drill => match point {
            Some(p) => def.tip_angle = p,
            None => {
                def.tip_angle = 118.0;
                approximations.push("no point angle: 118 degrees used".into());
            }
        },
        Kind::Tap => {
            def.pitch = pitch
                .filter(|p| *p > 0.0)
                .ok_or("it states no thread pitch")?;
            def.left_hand = type_word == "tap left hand";
        }
        Kind::ThreadMill => {
            if let Some(f) = &form {
                def.pitch = f.pitch;
                def.tip_angle = f.angle;
            } else {
                def.pitch = pitch
                    .filter(|p| *p > 0.0)
                    .ok_or("it states no thread pitch")?;
                def.tip_angle = profile.unwrap_or_else(|| {
                    approximations.push("no thread profile angle: 60 degrees used".into());
                    60.0
                });
            }
        }
        Kind::Bore => def.nose_radius = corner.unwrap_or(0.0),
        Kind::Turning => def.nose_radius = corner.ok_or("it states no nose radius")?,
        Kind::Groove => def.width = width.ok_or("it states no blade width")?,
        Kind::Thread => {
            def.width = width
                .or_else(|| pitch.map(|p| round6(0.866 * p)))
                .ok_or("it states no insert width or pitch")?;
            def.tip_angle = profile.unwrap_or(60.0);
            def.pitch = pitch.unwrap_or(0.0);
        }
        Kind::Form => {
            def.width = width.ok_or("it states no blade width")?;
            def.nose_radius = corner.ok_or("it states no crown radius")?;
        }
        Kind::Probe => {
            def.corner_r = round6(diameter / 2.0);
            if def.flute_len < diameter {
                return Err(format!(
                    "its stylus length {} mm is shorter than its {diameter} mm ball",
                    def.flute_len
                ));
            }
        }
        Kind::Torch | Kind::Waterjet => {}
        Kind::Keyseat | Kind::Lollipop => {}
    }
    if type_word == "face mill" {
        def.center_cutting = false;
    }

    cutting_data(src, &mut def, approximations, notes)?;
    def.tool_material = match src.tool.get("BMC").and_then(Value::as_str) {
        Some(b) => {
            src.mark("BMC".into());
            match b.trim().to_ascii_lowercase().as_str() {
                "carbide" => Some("carbide"),
                "hss" | "high speed steel" => Some("hss"),
                "" | "unspecified" => None,
                other => {
                    notes.push(format!(
                        "cutting material \"{other}\" has no CamCraft word; left unstated"
                    ));
                    None
                }
            }
        }
        None => None,
    };
    let mut note_parts: Vec<String> = Vec::new();
    if let Some(n) = number {
        note_parts.push(format!("Fusion tool number {n}"));
    }
    if let Some(link) = src.text("product-link") {
        note_parts.push(link);
    }
    if let Some(c) = src
        .section("post-process")
        .and_then(|p| p.get("comment"))
        .and_then(Value::as_str)
        .and_then(clean_text)
    {
        src.mark("post-process.comment".into());
        note_parts.push(c);
    }
    if !note_parts.is_empty() {
        def.notes = Some(trim_to(&note_parts.join("; "), 500));
    }
    nudge_dovetail(&mut def, notes);
    if let Some(why) = def.refusal() {
        return Err(format!("CamCraft would refuse it: {why}"));
    }
    Ok(def)
}

fn trim_to(s: &str, max: usize) -> String {
    s.chars().take(max).collect::<String>().trim().to_owned()
}

/// The holder as one cylinder: the widest segment, and the segments' total height.
fn holder_of(src: &Src<'_>, notes: &mut Vec<String>) -> (f64, f64) {
    let Some(holder) = src.tool.get("holder").and_then(Value::as_object) else {
        notes.push("no holder: CamCraft checks the tool without one".into());
        return (0.0, 0.0);
    };
    src.mark("holder.segments".into());
    src.mark("holder.unit".into());
    // A holder's segments are in millimetres in Fusion's exports, whatever the tool's unit,
    // unless the holder says its own unit.
    let to_mm = match holder.get("unit").and_then(Value::as_str) {
        Some("inches") => 25.4,
        _ => 1.0,
    };
    let mut width: f64 = 0.0;
    let mut height = 0.0;
    if let Some(Value::Array(segments)) = holder.get("segments") {
        for s in segments {
            for key in ["upper-diameter", "lower-diameter"] {
                if let Some(d) = s.get(key).and_then(number) {
                    width = width.max(d * to_mm);
                }
            }
            if let Some(h) = s.get("height").and_then(number) {
                height += h * to_mm;
            }
        }
    }
    if width <= 0.0 || height <= 0.0 {
        notes.push("the holder states no segments: CamCraft checks the tool without one".into());
        return (0.0, 0.0);
    }
    notes.push(
        "the holder's segments are written as one cylinder (its widest diameter and total height)"
            .into(),
    );
    (round6(width), round6(height))
}

/// A preset that states speed and feed: name, category, rpm, feed, stepdown, stepover.
type UsablePreset = (String, Option<String>, f64, f64, Option<f64>, Option<f64>);

/// Spindle speed, feed and per-material rows from the presets.
fn cutting_data(
    src: &Src<'_>,
    def: &mut Def,
    approximations: &mut Vec<String>,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    let presets: Vec<&Value> = src
        .tool
        .get("start-values")
        .and_then(|s| s.get("presets"))
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if !presets.is_empty() {
        src.mark("start-values.presets".into());
    }
    // (name, category, rpm, feed, stepdown, stepover) of every preset that states speed and feed.
    let mut usable: Vec<UsablePreset> = Vec::new();
    let mut turning: Vec<Option<(f64, f64)>> = Vec::new();
    for (i, p) in presets.iter().enumerate() {
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .and_then(clean_text)
            .unwrap_or_else(|| format!("preset {}", i.saturating_add(1)));
        let mut rpm = p.get("n").and_then(number).unwrap_or(0.0);
        let get = |key: &str| p.get(key).and_then(number).unwrap_or(0.0);
        // Constant surface speed (lathe): the preset names a surface speed and a feed per
        // revolution, and no spindle speed. Fusion's inch surface speed is feet per minute.
        let vc = get("v_c") * if src.to_mm > 1.0 { 0.3048 } else { 1.0 };
        let fpr = get("f_n") * src.to_mm;
        let css = def.kind.lathe_or_bore() && vc > 0.0 && fpr > 0.0;
        let per_rev = p
            .get("use-feed-per-revolution")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || (p.get("v_f").is_none() && p.get("f_n").is_some());
        if css && rpm <= 0.0 {
            rpm = 1000.0;
            notes.push(
                "the spindle speed is constant surface speed: its surface speed and feed per revolution are written as the tool's turning data, and 1000 rpm as its starting spindle speed"
                    .into(),
            );
        }
        let mut feed = if per_rev {
            get("f_n") * src.to_mm * rpm
        } else {
            get("v_f") * src.to_mm
        };
        if feed <= 0.0 {
            if let Some(z) = def.flutes {
                feed = get("f_z") * src.to_mm * f64::from(z) * rpm;
            }
        }
        if feed <= 0.0 {
            // Drills and taps state their feed as the plunge feed; a thread cut feeds one
            // pitch per revolution.
            feed = get("v_f_plunge") * src.to_mm;
        }
        if feed <= 0.0 && matches!(def.kind, Kind::Tap | Kind::Thread | Kind::ThreadMill) {
            feed = def.pitch * rpm;
        }
        let category = p
            .get("material")
            .and_then(|m| m.get("category"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case("all"))
            .map(str::to_owned);
        let step = |on: &str, key: &str| {
            p.get(on)
                .and_then(Value::as_bool)
                .filter(|b| *b)
                .and_then(|_| p.get(key).and_then(number))
                .map(|v| round6(v * src.to_mm))
                .filter(|v| *v > 0.0)
        };
        let (stepdown, stepover) = (
            step("use-stepdown", "stepdown"),
            step("use-stepover", "stepover"),
        );
        if (rpm > 0.0 && feed > 0.0) || (def.kind.spindleless() && feed > 0.0) {
            turning.push(css.then(|| (round6(vc), round6(fpr))));
            usable.push((
                name,
                category,
                round6(rpm),
                round6(feed),
                stepdown,
                stepover,
            ));
        } else {
            notes.push(format!(
                "preset \"{name}\" states no speed and feed and was not carried"
            ));
        }
    }
    let lead = usable
        .iter()
        .position(|u| u.1.is_none())
        .or(if usable.is_empty() { None } else { Some(0) });
    match lead.and_then(|i| usable.get(i)) {
        Some((_, category, rpm, feed, _, _)) => {
            def.rpm = if def.kind.spindleless() { 0.0 } else { *rpm };
            def.feed = *feed;
            def.material_group = category
                .clone()
                .map_or_else(|| "all".to_owned(), |c| c.to_ascii_lowercase());
            def.turning = lead.and_then(|i| turning.get(i).copied().flatten());
        }
        None => {
            let text =
                "no cutting data in the library: 1000 rpm and 100 mm/min were written as placeholders; set real values in CamCraft"
                    .to_owned();
            // A tool the source gives no presets has lost nothing: that is a note. Presets the
            // source did give and the mapping could not use are a change, so they stay degraded.
            if presets.is_empty() {
                notes.push(text);
            } else {
                approximations.push(text);
            }
            def.rpm = if def.kind.spindleless() { 0.0 } else { 1000.0 };
            def.feed = 100.0;
            def.material_group = "all".into();
        }
    }
    if def.kind == Kind::Probe {
        def.rpm = 1.0;
    }
    if def.kind.spindleless() || def.kind == Kind::Probe {
        if usable.len() > 1 {
            notes.push(
                "only the first preset's feed is used; this kind takes no per-material rows".into(),
            );
        }
        return Ok(());
    }
    let mut extra_defaults = 0;
    for (i, (name, category, rpm, feed, stepdown, stepover)) in usable.iter().enumerate() {
        let Some(category) = category else {
            if Some(i) != lead {
                extra_defaults += 1;
            }
            continue;
        };
        let Some(word) = material_word(category) else {
            notes.push(format!("preset \"{name}\" (material \"{category}\") matches no CamCraft material group; not carried"));
            continue;
        };
        if def.rows.iter().any(|r| r.material == word) {
            notes.push(format!(
                "preset \"{name}\" repeats material group {word}; not carried"
            ));
            continue;
        }
        def.rows.push(Row {
            material: word,
            rpm: *rpm,
            feed: *feed,
            stepdown: *stepdown,
            stepover: *stepover,
        });
    }
    if extra_defaults > 0 {
        notes.push(format!(
            "{extra_defaults} further preset(s) for all materials are not carried: CamCraft keeps one default"
        ));
    }
    Ok(())
}

/// Field paths holding data nothing above read.
fn unread(src: &Src<'_>) -> Vec<String> {
    let read = src.read.borrow();
    let mut out = Vec::new();
    let Some(root) = src.tool.as_object() else {
        return out;
    };
    for (key, value) in root {
        if read.contains(key) || QUIET_TOP.contains(&key.as_str()) {
            continue;
        }
        match (key.as_str(), value) {
            ("geometry" | "post-process" | "holder", Value::Object(section)) => {
                for (k, v) in section {
                    let path = format!("{key}.{k}");
                    if read.contains(&path) || QUIET_SECTION.contains(&k.as_str()) || !meaningful(v)
                    {
                        continue;
                    }
                    if key == "post-process"
                        && matches!(k.as_str(), "diameter-offset" | "length-offset")
                    {
                        // Fusion's own default: the offset is the tool number.
                        let same = section.get("number").and_then(number) == number_of(v);
                        if same {
                            continue;
                        }
                    }
                    // The tool number is carried in the notes; `live` is true on every Fusion mill.
                    if key == "post-process"
                        && (k == "number" || (k == "live" && v == &Value::Bool(true)))
                    {
                        continue;
                    }
                    out.push(path);
                }
            }
            ("start-values", Value::Object(sv)) => {
                for (k, v) in sv {
                    if k != "presets" && meaningful(v) {
                        out.push(format!("start-values.{k}"));
                    }
                }
                let mut seen = BTreeSet::new();
                for p in sv
                    .get("presets")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    for (k, v) in p.as_object().into_iter().flatten() {
                        if !matches!(
                            k.as_str(),
                            "n" | "v_f"
                                | "f_n"
                                | "f_z"
                                | "use-feed-per-revolution"
                                | "material"
                                | "stepdown"
                                | "stepover"
                                | "use-stepdown"
                                | "use-stepover"
                                | "v_c"
                                | "use-constant-surface-speed"
                        ) && !QUIET_SECTION.contains(&k.as_str())
                            && meaningful(v)
                        {
                            seen.insert(format!("preset.{k}"));
                        }
                    }
                }
                out.extend(seen);
            }
            _ if meaningful(value) => out.push(key.clone()),
            _ => {}
        }
    }
    out
}

fn number_of(v: &Value) -> Option<f64> {
    number(v)
}

/// What a form mill's profile draws when it is a multi-form thread mill.
struct FormTeeth {
    /// Across the tooth crests, mm.
    crest_d: f64,
    pitch: f64,
    /// Included flank angle, degrees.
    angle: f64,
    rows: usize,
    /// The widest diameter at the top of the profile (the shank), mm.
    shank_d: f64,
}

/// Fusion's form mill stores a profile of `{"end": [radius, height]}` points from the axis at
/// the tip, round and back to the axis. CamCraft's library has no place for a drawn profile; the
/// one shape it names is a multi-form thread mill (rows of equal 60-degree-style teeth at one
/// pitch), so that is read, and every other profile is refused by what it draws.
fn read_form(src: &Src<'_>) -> Result<FormTeeth, String> {
    const EPS: f64 = 1e-5;
    let raw = src
        .tool
        .get("geometry")
        .and_then(|g| g.get("profile"))
        .and_then(Value::as_array)
        .ok_or("a form mill states no profile")?;
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for seg in raw {
        let o = seg
            .as_object()
            .ok_or("its profile has a segment that is not a point")?;
        if o.len() != 1 {
            return Err("its profile has curved segments CamCraft's library cannot hold".into());
        }
        let e = o
            .get("end")
            .and_then(Value::as_array)
            .ok_or("its profile has a segment without an end point")?;
        let [r, z] = e.as_slice() else {
            return Err("its profile has a segment without an end point".into());
        };
        let (r, z) = (
            number(r).ok_or("a profile radius is not a number")?,
            number(z).ok_or("a profile height is not a number")?,
        );
        pts.push((r * src.to_mm, z * src.to_mm));
    }
    let axis_err = "its profile does not run from the axis at the tip to the axis above";
    let [first, inner @ .., last] = pts.as_slice() else {
        return Err(axis_err.into());
    };
    if inner.len() < 3 || *first != (0.0, 0.0) || last.0 != 0.0 {
        return Err(axis_err.into());
    }
    let (Some(&(root, root_z)), Some(&(top, _))) = (inner.first(), inner.last()) else {
        return Err(axis_err.into());
    };
    if root_z.abs() > EPS || root <= 0.0 {
        return Err("its profile does not start at the tip".into());
    }
    // Teeth: climbs out of the root radius at one angle to one crest radius.
    let mut teeth: Vec<(f64, f64, f64)> = Vec::new(); // (start height, crest radius, flank angle)
    for pair in inner.windows(2) {
        let [a, b] = *pair else { continue };
        let (dr, dz) = (b.0 - a.0, b.1 - a.1);
        if dr > EPS && (a.0 - root).abs() < EPS && dz > EPS {
            let ang = dz.atan2(dr).to_degrees();
            if let Some(&(_, c0, a0)) = teeth.first() {
                if (b.0 - c0).abs() > EPS || (ang - a0).abs() > 1e-3 {
                    break; // the neck's step out to the shank
                }
            }
            teeth.push((a.1, b.0, ang));
        } else if dr < -EPS {
            if let Some(&(_, _, a0)) = teeth.first() {
                let down = dz.atan2(-dr).to_degrees();
                if (down - a0).abs() > 1e-3 && (b.0 - root).abs() < EPS {
                    return Err("its teeth are asymmetric: the two flanks differ in angle".into());
                }
            }
        }
    }
    let Some(&(_, crest_r, flank)) = teeth.first() else {
        return Err("its profile draws no tooth".into());
    };
    if teeth.len() == 1 {
        return Err(format!(
            "its profile is one tooth of {} degrees included: CamCraft's single-form thread mill needs a pitch range Fusion does not store, and a form that is not a thread has no place in a library",
            round6(2.0 * flank)
        ));
    }
    let gaps: Vec<f64> = teeth
        .windows(2)
        .filter_map(|w| match *w {
            [a, b] => Some(b.0 - a.0),
            _ => None,
        })
        .collect();
    let pitch = round6(gaps.first().copied().unwrap_or(0.0));
    if pitch <= 0.0 || gaps.iter().any(|g| (g - pitch).abs() > EPS) {
        return Err("its tooth rows are not at one pitch".into());
    }
    Ok(FormTeeth {
        crest_d: round6(2.0 * crest_r),
        pitch,
        angle: round6(2.0 * flank),
        rows: teeth.len(),
        shank_d: round6(2.0 * top),
    })
}

/// A tapered mill's tip: CamCraft's cone ends in a sharp point, so only a tip of zero across
/// (no flat, no ball) is the same tool. Returns the diameter its cone reaches at the cutting
/// length.
fn tapered_tip(src: &Src<'_>) -> Result<f64, String> {
    let tip = src.len("geometry", &["DC", "diameter"]).unwrap_or(0.0);
    let ball = src.len("geometry", &["RE", "corner-radius"]).unwrap_or(0.0);
    if tip > 0.0 || ball > 0.0 {
        return Err(format!(
            "its tip is {tip} mm across with a {ball} mm corner radius: CamCraft's cone ends in a sharp point, so a flat or ball tip has no place"
        ));
    }
    let flute = src
        .len("geometry", &["LCF", "flute-length", "LE"])
        .filter(|v| *v > 0.0)
        .ok_or("it states no flute (cutting) length")?;
    let ta = src
        .raw("geometry", &["TA", "taper-angle"])
        .filter(|t| *t > 0.0 && *t < 90.0)
        .ok_or("it states no taper angle")?;
    Ok(round6(2.0 * flute * ta.to_radians().tan()))
}

/// CamCraft compares a dovetail's flank to its neck with no tolerance, so a flank that meets the
/// neck exactly can be refused for float dust. Shave the neck by the dust (never more than a
/// nanometre) so the exact tool is taken.
fn nudge_dovetail(def: &mut Def, notes: &mut Vec<String>) {
    if def.kind != Kind::Dovetail {
        return;
    }
    let (r, sr) = (def.diameter * 0.5, def.shank_d * 0.5);
    let top = r - def.flute_len / def.tip_angle.to_radians().tan();
    if top < sr && sr - top < 1e-9 {
        def.shank_d = 2.0 * top - 1e-12;
        notes.push("the neck meets the head's flank exactly; it is shaved by under a nanometre so CamCraft's exact comparison takes it".into());
    }
}
