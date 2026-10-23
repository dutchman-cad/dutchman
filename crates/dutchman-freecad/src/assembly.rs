//! Assembly workbench objects, using documented connector properties and an observed
//! FreeCAD 1.1 scripting interface. No FreeCAD implementation code is used here.
use std::collections::{BTreeMap, BTreeSet};

use dutchman_core::placement::{column, compose, IDENTITY};
use dutchman_core::{Assembly, Error, Model, Motion, Report, Severity, MAX_PLACEMENTS};
use serde_json::{json, Value};

use crate::generate;

const FACING: [[f64; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, -1.0, 0.0, 0.0],
    [0.0, 0.0, -1.0, 0.0],
];

fn note(report: &mut Report, index: usize, reason: &str) {
    report.push(
        Severity::Degraded,
        "joints",
        format!(
            "FreeCAD joint {}: {reason}; stored part placements are retained",
            index + 1
        ),
    );
}

/// Emit the assembly only after all of its occurrence containers have been written.
pub(crate) fn finish(
    model: &Model,
    assembly: &Assembly,
    script: &mut String,
    report: &mut Report,
    import_only: bool,
) -> Result<(), Error> {
    if assembly.placements.len() > MAX_PLACEMENTS
        || assembly.joints.len() > MAX_PLACEMENTS
        || assembly.held.len() > MAX_PLACEMENTS
        || model.ir.model.occurrences.len() > MAX_PLACEMENTS
        || model.ir.model.assembly_joints.len() > MAX_PLACEMENTS
    {
        return Err(Error::Encode(
            "assembly size exceeds the macro limit".to_owned(),
        ));
    }
    let mut rows = Vec::new();
    let mut held = assembly.held.clone();
    if import_only {
        if !assembly.joints.is_empty() || !model.ir.model.assembly_joints.is_empty() {
            report.push(
                Severity::Info,
                "joints",
                "FreeCAD joints left out: geometry only was requested",
            );
        }
        held.clear();
    } else {
        // The converted joints carry the same resolved paths, frames and motions as the
        // SolidCraft output, through sub-assemblies too.
        for (index, joint) in assembly.joints.iter().enumerate() {
            match converted(joint, index) {
                Ok(Converted::Pair(row)) => rows.push(row),
                Ok(Converted::Ground(path)) => held.push(path),
                Err(reason) => note(report, index, &reason),
            }
        }
        // The converted core type has only three motion families. Read the neutral wire for
        // the other workbench families; never decode a native file here. A direct
        // occurrence correspondence is required, not guessed.
        let wire =
            serde_json::to_value(&model.ir).map_err(|error| Error::Encode(error.to_string()))?;
        let raw = wire
            .pointer("/model/assembly_joints")
            .and_then(Value::as_array);
        let raw = raw.map_or(&[][..], Vec::as_slice);
        let paths = direct_paths(&wire, assembly);
        for (index, row) in raw.iter().enumerate() {
            let definition = row.pointer("/operands/kind");
            let family = definition
                .and_then(|kind| kind.get("kind"))
                .and_then(Value::as_str);
            let grounded =
                row.pointer("/operands/arity").and_then(Value::as_str) == Some("grounded");
            if grounded || matches!(family, Some("fixed" | "revolute" | "slider")) {
                // Written from the converted joint above.
                if family == Some("fixed")
                    && definition.is_some_and(|kind| {
                        kind.get("angular_limits").is_some() || kind.get("linear_limits").is_some()
                    })
                {
                    note(
                        report,
                        index,
                        "limits on a fixed joint have no FreeCAD degree of freedom to constrain",
                    );
                }
            } else {
                match raw_joint(row, &paths, &assembly.joints) {
                    Ok((Some(mapped), None)) => rows.push(mapped),
                    Ok((None, Some(path))) => held.push(path),
                    Ok(_) => {}
                    Err(reason) => note(report, index, &reason),
                }
            }
            if row.get("driving_parameter").is_some() {
                report.push(Severity::Degraded, "joints", format!("FreeCAD joint {}: driving parameter expressions are not supported yet; only evaluated values can be kept", index + 1));
            }
        }
    }
    let scopes = scopes(rows, held, report);
    let written = scopes.iter().any(|scope| {
        scope
            .get("joints")
            .and_then(Value::as_array)
            .is_some_and(|joints| !joints.is_empty())
    });
    script.push_str("finish_assembly(assembly_scopes.pop(), ");
    script.push_str(&generate::quoted(&model.name)?);
    script.push_str(", ");
    script.push_str(&generate::data(&json!({ "scopes": scopes }))?);
    script.push_str(")\n");
    if written {
        report.push(Severity::Info, "joints", "FreeCAD Assembly joints are unverified until the macro runs; a failed solve removes them and restores the stored placements");
    }
    Ok(())
}

enum Converted {
    Pair(Value),
    Ground(Vec<usize>),
}

/// A converted joint as a workbench row with paths from this assembly, or the part it holds
/// to the assembly itself.
fn converted(joint: &dutchman_core::Joint, index: usize) -> Result<Converted, String> {
    if joint.a.is_empty() || joint.b.is_empty() {
        let path = if joint.a.is_empty() {
            &joint.b
        } else {
            &joint.a
        };
        if joint.motion == Motion::Rigid && !path.is_empty() {
            if joint.suppressed {
                return Err("suppressed grounding is left out; FreeCAD GroundedJoint has no suppression property".to_owned());
            }
            return Ok(Converted::Ground(path.clone()));
        }
        return Err(
            "moving or suppressed joints to the assembly itself are not supported yet".to_owned(),
        );
    }
    let (kind, angular) = match &joint.motion {
        Motion::Rigid => ("Fixed", None),
        Motion::Revolute { limits, .. } => ("Revolute", *limits),
        Motion::Slider { .. } => ("Slider", None),
    };
    Ok(Converted::Pair(json!({
        "name": format!("Joint {}", index + 1), "kind": kind,
        "a": joint.a, "b": joint.b, "frame_a": joint.frame_a,
        "frame_b": if joint.flip { joint.frame_b } else { compose(&joint.frame_b, &FACING) },
        "suppressed": joint.suppressed,
        "angular": angular.map(|(minimum, maximum)| json!({"minimum": minimum.to_degrees(), "maximum": maximum.to_degrees()})),
    })))
}

/// Match direct local occurrences in the same stable ordinal order as local_assembly.
/// Nested/external operands require a separate reference resolver and are refused below.
fn direct_paths(wire: &Value, assembly: &Assembly) -> BTreeMap<String, Vec<usize>> {
    let mut roots: Vec<&Value> = wire
        .pointer("/model/occurrences")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.pointer("/parent/kind").and_then(Value::as_str) == Some("root"))
        .collect();
    roots.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));
    let mut out = BTreeMap::new();
    for (index, row) in roots.into_iter().enumerate() {
        if row.pointer("/prototype/scope").and_then(Value::as_str) != Some("local") {
            return BTreeMap::new();
        }
        let Some(placement) = assembly.placements.get(index) else {
            return BTreeMap::new();
        };
        if row.get("name").and_then(Value::as_str) != placement.name.as_deref() {
            return BTreeMap::new();
        }
        if let Some(id) = row.get("id").and_then(Value::as_str) {
            out.insert(id.to_owned(), vec![index]);
        }
    }
    out
}

fn operand(connector: &Value, paths: &BTreeMap<String, Vec<usize>>) -> Result<Vec<usize>, String> {
    let operand = connector
        .get("operand")
        .ok_or("the connector has no operand")?;
    let container = operand
        .get("container")
        .ok_or("the operand has no container")?;
    if container.get("container").and_then(Value::as_str) != Some("occurrence") {
        return Err(
            "root and external CADIR operands are not supported yet in FreeCAD output".to_owned(),
        );
    }
    if container
        .get("occurrence_path")
        .and_then(Value::as_array)
        .is_some_and(|v| !v.is_empty())
        || operand
            .get("subelements")
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty())
    {
        return Err(
            "nested or subelement joint operands are not supported yet in FreeCAD output"
                .to_owned(),
        );
    }
    let id = container
        .get("occurrence")
        .and_then(Value::as_str)
        .ok_or("the operand names no occurrence")?;
    paths
        .get(id)
        .cloned()
        .ok_or_else(|| "the joint occurrence cannot be matched to a direct placed part".to_owned())
}

fn matrix(value: Option<&Value>) -> Result<[[f64; 4]; 3], String> {
    let rows = value
        .and_then(Value::as_array)
        .filter(|rows| rows.len() == 3)
        .ok_or("the connector frame must have three rows")?;
    let mut out = IDENTITY;
    for (target, row) in out.iter_mut().zip(rows) {
        let values = row
            .as_array()
            .filter(|row| row.len() == 4)
            .ok_or("the connector frame must have four columns")?;
        for (target, value) in target.iter_mut().zip(values) {
            *target = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or("the connector frame must be finite")?;
        }
    }
    let axes = [column(&out, 0), column(&out, 1), column(&out, 2)];
    for (i, a) in axes.iter().enumerate() {
        for (j, b) in axes.iter().enumerate() {
            let dot: f64 = a.iter().zip(b).map(|(a, b)| a * b).sum();
            let want = if i == j { 1.0 } else { 0.0 };
            if (dot - want).abs() > 1e-8 {
                return Err("scaled or skewed connector frames are not supported yet".to_owned());
            }
        }
    }
    let [[a, b, c, _], [d, e, f, _], [g, h, i, _]] = out;
    if a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g) < 0.0 {
        return Err("reflected connector frames are not supported yet".to_owned());
    }
    Ok(out)
}

fn bounds(value: Option<&Value>, angular: bool) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    let mut out = serde_json::Map::new();
    for key in ["minimum", "maximum"] {
        if let Some(number) = value.get(key).and_then(Value::as_f64) {
            out.insert(
                key.to_owned(),
                json!(if angular { number.to_degrees() } else { number }),
            );
        }
    }
    Value::Object(out)
}

// The tuple distinguishes a paired joint from a grounded occurrence.
type MappedJoint = (Option<Value>, Option<Vec<usize>>);

fn raw_joint(
    row: &Value,
    paths: &BTreeMap<String, Vec<usize>>,
    converted: &[dutchman_core::Joint],
) -> Result<MappedJoint, String> {
    let operands = row.get("operands").ok_or("the joint has no operands")?;
    let suppressed = row
        .get("suppressed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if operands.get("arity").and_then(Value::as_str) == Some("grounded") {
        let connector = operands
            .get("connector")
            .ok_or("the ground has no connector")?;
        let path = operand(connector, paths)?;
        if suppressed {
            return Err("suppressed grounding is left out; FreeCAD GroundedJoint has no suppression property".to_owned());
        }
        return Ok((None, Some(path)));
    }
    let definition = operands.get("kind").ok_or("the joint has no family")?;
    let family = definition
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let kind = match family {
        "fixed" => "Fixed",
        "revolute" => "Revolute",
        "slider" => "Slider",
        "cylindrical" | "axis_coincidence" => "Cylindrical",
        "insert" => "Revolute",
        "ball" => "Ball",
        "distance" => "Distance",
        "parallel" => "Parallel",
        "perpendicular" => "Perpendicular",
        "angle" => "Angle",
        "rack_pinion" | "screw" | "gears" | "belt" => {
            return Err(format!("{family} coupling needs resolved pitch/radius and driving joint references, which CADIR does not carry"));
        }
        "plane_coincidence" => {
            return Err("plane_coincidence needs a planar joint with free in-plane translation; FreeCAD Assembly has no planar joint".to_owned());
        }
        "point_on_axis" => {
            return Err("point_on_axis needs point-to-line distance semantics; FreeCAD Assembly has no point-on-axis joint".to_owned());
        }
        "tangent" => {
            return Err("tangent needs a resolved cylindrical face radius and plane contact; FreeCAD Assembly has no tangent joint".to_owned());
        }
        "native" => {
            return Err(format!(
                "native joint family {:?} is not supported yet in FreeCAD output",
                definition
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            ));
        }
        other => {
            return Err(format!(
                "{other} joint family is not supported yet in FreeCAD output"
            ));
        }
    };
    if family == "fixed"
        && (definition.get("angular_limits").is_some() || definition.get("linear_limits").is_some())
    {
        return Err(
            "limits on a fixed joint have no FreeCAD degree of freedom to constrain".to_owned(),
        );
    }
    let connectors = operands
        .get("connectors")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 2)
        .ok_or("a paired joint needs two connectors")?;
    let a = connectors.first().ok_or("the first connector is missing")?;
    let b = connectors.get(1).ok_or("the second connector is missing")?;
    let path_a = operand(a, paths)?;
    let path_b = operand(b, paths)?;
    if path_a == path_b {
        return Err("a joint joining an occurrence to itself is not supported yet".to_owned());
    }
    let mut frame_a = matrix(a.get("frame"))?;
    let mut frame_b = matrix(b.get("frame"))?;
    if let Some(offsets) = operands.get("offset_frames").and_then(Value::as_array) {
        frame_a = compose(&frame_a, &matrix(offsets.first())?);
        frame_b = compose(&frame_b, &matrix(offsets.get(1))?);
    }
    // CADIR mates opposed frames; FreeCAD superimposes its connectors. Apply the
    // half turn to B only, leaving the first frame's motion axis and limit sign intact.
    let aligned = if family == "insert" {
        definition.get("sense").and_then(Value::as_str) == Some("aligned")
    } else {
        converted
            .iter()
            .find(|joint| joint.a == path_a && joint.b == path_b)
            .map(|joint| joint.flip)
            .unwrap_or_else(|| row.get("frame_sense").and_then(Value::as_str) == Some("aligned"))
    };
    if !aligned {
        frame_b = compose(&frame_b, &FACING);
    }
    let number = |key: &str| definition.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let mut offset = IDENTITY;
    if matches!(family, "fixed" | "revolute" | "cylindrical") {
        let (sin, cos) = number("angle").sin_cos();
        offset = [
            [cos, -sin, 0.0, 0.0],
            [sin, cos, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
    }
    if let Some(translation) = definition
        .get("translation_offset")
        .and_then(Value::as_array)
    {
        for (row, value) in offset.iter_mut().zip(translation) {
            if let Some((cell, number)) = row.get_mut(3).zip(value.as_f64()) {
                *cell = number;
            }
        }
    }
    if let ("insert", Some(z)) = (family, offset.get_mut(2).and_then(|row| row.get_mut(3))) {
        *z = -number("offset");
    }
    frame_a = compose(&frame_a, &offset);
    Ok((
        Some(json!({
            "name": row.get("id"), "kind": kind, "a": path_a, "b": path_b,
            "frame_a": frame_a, "frame_b": frame_b, "suppressed": suppressed,
            "angular": bounds(definition.get("angular_limits"), true),
            "linear": bounds(definition.get("linear_limits"), false),
            "angle": if family == "angle" { number("angle").to_degrees() } else { 0.0 },
            "distance": number("distance"),
            "axis_unoriented": family == "axis_coincidence",
        })),
        None,
    ))
}

fn path_of(row: &Value, key: &str) -> Vec<usize> {
    row.get(key)
        .and_then(Value::as_array)
        .map(|path| {
            path.iter()
                .filter_map(|index| index.as_u64().and_then(|v| usize::try_from(v).ok()))
                .collect()
        })
        .unwrap_or_default()
}

fn shown(path: &[usize]) -> String {
    path.iter()
        .map(|index| (index + 1).to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// A sub-assembly's joint rows and grounded parts.
type Scope = (Vec<Value>, Vec<Vec<usize>>);

/// Group joints and grounds by the deepest sub-assembly holding both sides: FreeCAD solves
/// each sub-assembly's own joints, and moves a sub-assembly as one body in its parent.
/// Paths inside a scope are relative to it, deepest scopes first.
fn scopes(rows: Vec<Value>, mut held: Vec<Vec<usize>>, report: &mut Report) -> Vec<Value> {
    let mut grouped: BTreeMap<Vec<usize>, Scope> = BTreeMap::new();
    for mut row in rows {
        let (a, b) = (path_of(&row, "a"), path_of(&row, "b"));
        let shared = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("a joint")
            .to_owned();
        if shared == a.len() || shared == b.len() {
            // One side is a sub-assembly holding the other.
            let (outer, inner) = if a.len() < b.len() {
                (&a, &b)
            } else {
                (&b, &a)
            };
            let fixed = row.get("kind").and_then(Value::as_str) == Some("Fixed")
                && row.get("suppressed").and_then(Value::as_bool) != Some(true);
            if fixed && inner.len() == outer.len() + 1 {
                held.push(inner.clone());
            } else {
                report.push(Severity::Degraded, "joints", format!("FreeCAD {name}: a joint between a sub-assembly and a part inside it is not supported yet; stored part placements are retained"));
            }
            continue;
        }
        let scope = a.get(..shared).map(<[usize]>::to_vec).unwrap_or_default();
        let local = |path: &[usize]| {
            path.get(shared..)
                .map(<[usize]>::to_vec)
                .unwrap_or_default()
        };
        let (local_a, local_b) = (local(&a), local(&b));
        if (local_a.len() > 1 || local_b.len() > 1)
            && row.get("kind").and_then(Value::as_str) != Some("Fixed")
        {
            report.push(Severity::Info, "joints", format!("FreeCAD {name} moves a whole sub-assembly: FreeCAD sub-assemblies are rigid in the assembly that holds them"));
        }
        if let Some(fields) = row.as_object_mut() {
            fields.insert("a".to_owned(), json!(local_a));
            fields.insert("b".to_owned(), json!(local_b));
        }
        grouped.entry(scope).or_default().0.push(row);
    }
    for path in held {
        if let Some((last, scope)) = path.split_last() {
            grouped
                .entry(scope.to_vec())
                .or_default()
                .1
                .push(vec![*last]);
        }
    }
    let mut out = Vec::new();
    for (scope, (joints, mut held)) in grouped {
        if joints.is_empty() {
            // Nothing in this sub-assembly moves; its parts stay where they are placed.
            continue;
        }
        infer_ground(&scope, &joints, &mut held, report);
        held.sort();
        held.dedup();
        out.push(json!({"path": scope, "joints": joints, "held": held}));
    }
    out.sort_by_key(|scope| {
        std::cmp::Reverse(
            scope
                .get("path")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        )
    });
    out
}

/// Grounds one part of each chain of active joints in a scope that nothing grounds: the
/// second side of the chain's first joint, as the SolidCraft output does.
fn infer_ground(scope: &[usize], rows: &[Value], held: &mut Vec<Vec<usize>>, report: &mut Report) {
    let active: Vec<(usize, usize)> = rows
        .iter()
        .filter(|row| row.get("suppressed").and_then(Value::as_bool) != Some(true))
        .filter_map(|row| Some((*path_of(row, "a").first()?, *path_of(row, "b").first()?)))
        .collect();
    let mut edges: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for (a, b) in &active {
        edges.entry(*a).or_default().insert(*b);
        edges.entry(*b).or_default().insert(*a);
    }
    let mut seen = BTreeSet::new();
    for (_, first) in &active {
        if seen.contains(first) {
            continue;
        }
        let mut pending = vec![*first];
        let mut component = BTreeSet::new();
        while let Some(at) = pending.pop() {
            if !seen.insert(at) {
                continue;
            }
            component.insert(at);
            if let Some(neighbors) = edges.get(&at) {
                pending.extend(neighbors);
            }
        }
        if !held
            .iter()
            .any(|path| path.first().is_some_and(|index| component.contains(index)))
        {
            held.push(vec![*first]);
            let mut path = scope.to_vec();
            path.push(*first);
            report.push(Severity::Info, "joints", format!("FreeCAD occurrence {} is grounded, inferred: its joint chain has no grounded part", shown(&path)));
        }
    }
}

/// JointObject's constructors and the solver were exercised in FreeCAD 1.1.4; the
/// property contract is documented on Assembly_CreateJointFixed and ToggleGrounded.
/// Keep this separate from the part prelude so part-only goldens remain unchanged.
pub(crate) const PRELUDE: &str = r#"
# Experimental Assembly workbench reconstruction.
assembly_scopes = []

def joint_placement(rows):
    matrix = App.Matrix()
    for i in range(3):
        for j in range(4):
            setattr(matrix, 'A%d%d' % (i + 1, j + 1), rows[i][j])
    return App.Placement(matrix)

def same_pose(a, b):
    return (a.Base - b.Base).Length <= 1e-7 and a.Rotation.isSame(b.Rotation, 1e-7)

def drive_joint_angle(assembly, joint, degrees):
    # A direct two-part hinge can be driven from its frozen connector frames, turning
    # whichever side is not grounded. Leave chains and loops to the workbench's simulation.
    if joint.JointType != 'Revolute' or joint.Suppressed or not math.isfinite(degrees):
        raise ValueError('Driving requires an active revolute joint and a finite angle')
    first, second = joint.Reference1[0], joint.Reference2[0]
    if len(assembly.Joints) != 1 or assembly.isPartGrounded(first) == assembly.isPartGrounded(second):
        raise ValueError('Driving requires one hinge with exactly one of its parts grounded')
    if joint.EnableAngleMin:
        degrees = max(degrees, joint.AngleMin.Value)
    if joint.EnableAngleMax:
        degrees = min(degrees, joint.AngleMax.Value)
    turn = App.Placement(App.Vector(), App.Rotation(App.Vector(0, 0, 1), degrees))
    if assembly.isPartGrounded(first):
        fixed, moving = first, second
        wanted = first.Placement.multiply(joint.Placement1).multiply(turn).multiply(joint.Placement2.inverse())
    else:
        fixed, moving = second, first
        wanted = second.Placement.multiply(joint.Placement2).multiply(turn.inverse()).multiply(joint.Placement1.inverse())
    held = App.Placement(fixed.Placement)
    before = App.Placement(moving.Placement)
    moving.Placement = wanted
    if assembly.solve() != 0 or not same_pose(fixed.Placement, held) or not same_pose(moving.Placement, wanted):
        fixed.Placement = held
        moving.Placement = before
        raise RuntimeError('The hinge could not reach the requested angle')
    doc.recompute()

# The occurrence containers of each sub-assembly, by the name of the container holding them.
assembly_children = {}

def scope_at(parts, path):
    owner, current = None, parts
    for index in path:
        owner = current[index]
        current = assembly_children.get(owner.Name, [])
    return owner, current

def joint_reference(children, path):
    # A part inside a sub-assembly is reached through it by object names.
    first = children[path[0]]
    if len(path) == 1:
        return (first, [''])
    target = scope_at(children, path[:-1])[1][path[-1]]
    names = []
    obj = target
    while obj is not None and obj != first:
        names.append(obj.Name)
        obj = obj.getParentGeoFeatureGroup()
    if obj is None:
        raise RuntimeError('A joined part is not inside its sub-assembly')
    return (first, ['.'.join(reversed(names)) + '.'])

def finish_assembly(parts, label, data):
    if parents:
        assembly_children[parents[-1].Name] = parts
    scopes = [scope for scope in data['scopes'] if scope['joints']]
    if not scopes:
        return
    saved = []
    created = []
    try:
        import JointObject
        built = []
        # Deepest sub-assemblies first: each is solved before the assembly that holds it.
        for scope in scopes:
            owner, children = scope_at(parts, scope['path'])
            assembly = doc.addObject('Assembly::AssemblyObject', 'Assembly')
            created.append(assembly)
            assembly.Label = label if owner is None else owner.Label
            if owner is None:
                attach(assembly)
            else:
                owner.addObject(assembly)
            group = assembly.newObject('Assembly::JointGroup', 'JointGroup')
            created.append(group)
            for part in children:
                saved.append((part, owner, App.Placement(part.Placement)))
                assembly.addObject(part)
            built.append((scope, assembly, group, children))
        for scope, assembly, group, children in built:
            for path in scope['held']:
                ground = group.newObject('App::FeaturePython', 'GroundedJoint')
                created.append(ground)
                JointObject.GroundedJoint(ground, children[path[0]])
            for row in scope['joints']:
                joint = group.newObject('App::FeaturePython', 'Joint')
                created.append(joint)
                JointObject.Joint(joint, JointObject.JointTypes.index(row['kind']))
                joint.Label = row['name'] or row['kind']
                joint.Suppressed = row['suppressed']
                joint.Reference1 = joint_reference(children, row['a'])
                joint.Reference2 = joint_reference(children, row['b'])
                joint.Detach1 = True
                joint.Detach2 = True
                joint.Placement1 = joint_placement(row['frame_a'])
                joint.Placement2 = joint_placement(row['frame_b'])
                if row.get('axis_unoriented'):
                    first = children[row['a'][0]].Placement.multiply(joint.Placement1)
                    second = children[row['b'][0]].Placement.multiply(joint.Placement2)
                    z = App.Vector(0, 0, 1)
                    if first.Rotation.multVec(z).dot(second.Rotation.multVec(z)) < 0:
                        joint.Placement2 = joint.Placement2.multiply(App.Placement(App.Vector(), App.Rotation(App.Vector(1, 0, 0), 180)))
                joint.Angle = row.get('angle', 0)
                joint.Distance = row.get('distance', 0)
                for key, stem in [('angular', 'Angle'), ('linear', 'Length')]:
                    limits = row.get(key) or {}
                    for bound, suffix in [('minimum', 'Min'), ('maximum', 'Max')]:
                        setattr(joint, 'Enable' + stem + suffix, bound in limits)
                        if bound in limits:
                            setattr(joint, stem + suffix, limits[bound])
        doc.recompute()
        for scope, assembly, group, children in built:
            if any(not row['suppressed'] for row in scope['joints']):
                status = assembly.solve()
                if status != 0:
                    raise RuntimeError('Assembly solver returned ' + str(status))
        if any(not same_pose(part.Placement, placement) for part, owner, placement in saved):
            raise RuntimeError('Assembly solve changed the stored part placements')
    except Exception as error:
        # Remove every constraint, then put each occurrence container back where it was.
        for obj in reversed(created):
            if obj.Document is not None:
                doc.removeObject(obj.Name)
        for part, owner, placement in saved:
            if owner is None:
                attach(part)
            else:
                owner.addObject(part)
            part.Placement = placement
        doc.recompute()
        App.Console.PrintWarning('Assembly joints failed; stored placements restored: ' + str(error) + '\n')
"#;
