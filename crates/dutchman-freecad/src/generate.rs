//! Emit data as escaped literals, never executable expressions from a design.
use dutchman_core::Error;
use serde_json::Value;

pub(crate) fn quoted(text: &str) -> Result<String, Error> {
    serde_json::to_string(text).map_err(|error| Error::Encode(error.to_string()))
}

/// Fixed scientific formatting preserves even very small model coordinates.
fn json(value: &Value, depth: usize) -> Result<String, Error> {
    if depth > 64 {
        return Err(Error::Encode(
            "macro data exceeds the nesting limit".to_owned(),
        ));
    }
    Ok(match value {
        Value::Number(n) if n.is_f64() => {
            let number = n
                .as_f64()
                .ok_or_else(|| Error::Encode("macro number is not finite".to_owned()))?;
            format!("{number:.17e}")
        }
        Value::Array(rows) => {
            let rows = rows
                .iter()
                .map(|row| json(row, depth + 1))
                .collect::<Result<Vec<_>, _>>()?;
            format!("[{}]", rows.join(","))
        }
        Value::Object(fields) => {
            let fields = fields
                .iter()
                .map(|(key, value)| {
                    Ok(format!("{}:{}", quoted(key)?, json(value, depth + 1)?))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            format!("{{{}}}", fields.join(","))
        }
        other => other.to_string(),
    })
}

pub(crate) fn data(value: &Value) -> Result<String, Error> {
    Ok(format!("json.loads({})", quoted(&json(value, 0)?)?))
}

pub(crate) fn history(rows: &[Value]) -> Result<String, String> {
    if rows.len() > 100_000 {
        return Err("more than 100000 mapped features are not supported yet".to_owned());
    }
    let mut code = "objects = {}\nnames = {}\ntip = None\n".to_owned();
    let mut names = std::collections::BTreeSet::new();
    for row in rows {
        let kind = row.get("type").and_then(Value::as_str).unwrap_or("unknown");
        let name = row.get("name").and_then(Value::as_str).unwrap_or(kind);
        if !names.insert(name) {
            return Err(format!(
                "feature {name:?}: duplicate feature names are not supported yet"
            ));
        }
        match kind {
            "sketch" | "extrude" | "revolve" | "loft" | "sweep" | "hole" | "pattern"
            | "sphere" | "torus" | "coil" => {}
            _ => {
                return Err(format!(
                    "feature {name:?}: {kind} features are not supported yet in FreeCAD output"
                ));
            }
        }
        if kind == "sketch"
            && ["points", "curves", "constraints"].iter().any(|key| {
                row.get("sketch")
                    .and_then(|sketch| sketch.get(*key))
                    .and_then(Value::as_array)
                    .is_some_and(|rows| rows.len() > 100_000)
            })
        {
            return Err(format!("feature {name:?}: sketch size exceeds the macro limit"));
        }
        if row.get("operation").and_then(Value::as_str) == Some("intersect") {
            return Err(format!(
                "feature {name:?}: intersect operations are not supported yet in FreeCAD output"
            ));
        }
        if row.get("orientation").and_then(Value::as_str) == Some("parallel") {
            return Err(format!(
                "feature {name:?}: fixed sweep orientation is not supported yet in FreeCAD output"
            ));
        }
        if row.get("rails").is_some() || row.get("twist").is_some() {
            return Err(format!(
                "feature {name:?}: guided or twisted features are not supported yet in FreeCAD output"
            ));
        }
        if kind == "pattern" {
            let layout = row.get("pattern").ok_or("pattern has no layout")?;
            if layout.get("count2").is_some() {
                return Err(format!(
                    "feature {name:?}: two-direction patterns are not supported yet in FreeCAD output"
                ));
            }
            let count = layout
                .get("count1")
                .or_else(|| layout.get("count"))
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<u64>().ok());
            if !count.is_some_and(|n| (1..=10_000).contains(&n)) {
                return Err(format!(
                    "feature {name:?}: pattern count exceeds the macro limit"
                ));
            }
        }
        code.push_str("tip = rebuild(");
        code.push_str(&data(row).map_err(|error| error.to_string())?);
        code.push_str(", body, objects, names, tip)\n");
    }
    Ok(code)
}

// Only public scripting interfaces are used. API references are recorded in docs/freecad.md.
pub(crate) const PRELUDE: &str = r#"# -*- coding: utf-8 -*-
# Dutchman FreeCAD rebuild macro. Original geometry is embedded for recovery.
import FreeCAD as App
import Part
import Sketcher
import json
import math
import os
import tempfile
if App.GuiUp:
    import FreeCADGui as Gui
doc = App.newDocument('Dutchman')
parents = []

def visible(obj, shown):
    if App.GuiUp:
        obj.ViewObject.Visibility = shown

def attach(obj):
    if parents:
        parents[-1].addObject(obj)

def container(label, rows):
    obj = doc.addObject('App::Part', 'Occurrence')
    obj.Label = label
    attach(obj)
    matrix = App.Matrix()
    for i in range(3):
        for j in range(4):
            setattr(matrix, 'A%d%d' % (i + 1, j + 1), rows[i][j])
    obj.Placement = App.Placement(matrix)
    return obj

def original_step(label, text):
    handle, path = tempfile.mkstemp(suffix='.step')
    try:
        with os.fdopen(handle, 'w', encoding='utf-8', newline='\n') as stream:
            stream.write(text)
        shape = Part.Shape()
        shape.read(path)
    finally:
        os.unlink(path)
    obj = doc.addObject('PartDesign::Body', 'OriginalBody')
    obj.Label = label + ' (original STEP)'
    attach(obj)
    base = obj.newObject('PartDesign::Feature', 'OriginalGeometry')
    base.Shape = shape
    doc.recompute()
    return obj

def compare_original(rebuilt, original):
    if len(rebuilt.Solids) != len(original.Solids):
        raise RuntimeError('Rebuilt solid count differs from original STEP')
    difference = rebuilt.cut(original)
    missing = original.cut(rebuilt)
    measure = 'Volume' if original.Solids else 'Area'
    tolerance = max(abs(getattr(original, measure)), 1.0) * 1e-7
    if abs(getattr(difference, measure)) + abs(getattr(missing, measure)) > tolerance:
        raise RuntimeError('Rebuilt geometry differs from original STEP')

def parameter_table(rows):
    obj = doc.addObject('App::FeaturePython', 'Parameters')
    attach(obj)
    for i, row in enumerate(rows):
        key = 'Parameter%d' % (i+1)
        obj.addProperty('App::PropertyString', key + 'Name', 'Parameters')
        setattr(obj, key + 'Name', row['name'])
        obj.addProperty('App::PropertyString', key + 'Expression', 'Parameters')
        setattr(obj, key + 'Expression', row.get('expression') or '')
        value = row.get('value') or {}
        scalar = value.get('value')
        if isinstance(scalar, (int, float)) and math.isfinite(scalar):
            obj.addProperty('App::PropertyFloat', key, 'Parameters')
            setattr(obj, key, scalar)

def quantity(value):
    # Quantities come from the mapper, never from source expressions.
    text = str(value).strip().strip('()').strip()
    if '/' in str(value):
        first, second = str(value).split('/', 1)
        return quantity(first) / quantity(second)
    return App.Units.Quantity(text).Value

def vector(value):
    return App.Vector(*value)

def plane(value):
    if value['type'] == 'origin':
        frames = {'XY': ([1,0,0], [0,1,0]), 'XZ': ([1,0,0], [0,0,-1]), 'YZ': ([0,1,0], [0,0,1])}
        x, y = map(vector, frames[value['name']])
        origin = App.Vector()
    else:
        frame = value['plane']
        x, y = vector(frame['x_dir']), vector(frame['y_dir'])
        origin = vector(frame['origin'])
    return App.Placement(origin, App.Rotation(x, y, x.cross(y), 'ZXY'))

def sketch(row, body):
    obj = body.newObject('Sketcher::SketchObject', 'Sketch')
    obj.Label = row['name']
    obj.Placement = plane(row['plane'])
    data = row['sketch']
    points = [App.Vector(*p['pos'], 0) for p in data['points']]
    endpoints = {0: (-1, 1)}
    shared = []
    def endpoint(point, location):
        if point in endpoints:
            shared.append((endpoints[point], location))
        else:
            endpoints[point] = location
    for i, curve in enumerate(data['curves']):
        kind = curve['type']
        if kind == 'line':
            geom = Part.LineSegment(points[curve['a']], points[curve['b']])
            endpoint(curve['a'], (i, 1))
            endpoint(curve['b'], (i, 2))
        elif kind in ('circle', 'arc'):
            center = points[curve['c']]
            if kind == 'circle':
                geom = Part.Circle(center, App.Vector(0,0,1), curve['r'])
            else:
                start, end = points[curve['a']], points[curve['b']]
                radius = (start-center).Length
                a = math.atan2(start.y-center.y, start.x-center.x)
                b = math.atan2(end.y-center.y, end.x-center.x)
                while b <= a:
                    b += 2 * math.pi
                geom = Part.ArcOfCircle(Part.Circle(center, App.Vector(0,0,1), radius), a, b)
                endpoint(curve['a'], (i, 1))
                endpoint(curve['b'], (i, 2))
            endpoint(curve['c'], (i, 3))
        elif kind == 'ellipse':
            center, major = points[curve['c']], points[curve['m']]
            delta = major-center
            minor = center + App.Vector(-delta.y, delta.x, 0).normalize() * curve['r']
            geom = Part.Ellipse(major, minor, center)
            endpoint(curve['c'], (i, 3))
        elif kind == 'spline':
            poles = [points[p] for p in curve['pts']]
            degree = curve['degree']
            spans = len(poles)-degree
            knots = [float(k) for k in range(spans+1)]
            mults = [degree+1] + [1]*(spans-1) + [degree+1]
            geom = Part.BSplineCurve()
            geom.buildFromPolesMultsKnots(poles, mults, knots, False, degree)
        else:
            raise RuntimeError('Sketch curve is not supported: ' + kind)
        obj.addGeometry(geom, curve.get('construction', False))
    for i, point in enumerate(points):
        if i not in endpoints:
            endpoints[i] = (obj.addGeometry(Part.Point(point), True), 1)
    for first, second in shared:
        obj.addConstraint(Sketcher.Constraint('Coincident', *first, *second))
    for constraint in data['constraints']:
        add_constraint(obj, constraint, endpoints)
    doc.recompute()
    return obj

def add_constraint(obj, c, points):
    kind = c['type']
    single = {'horizontal':'Horizontal', 'vertical':'Vertical'}
    pairs = {'parallel':'Parallel', 'perpendicular':'Perpendicular', 'tangent':'Tangent', 'equal':'Equal'}
    if kind in single:
        args = [single[kind], c['l']]
    elif kind in pairs:
        args = [pairs[kind], c['a'], c['b']]
    elif kind == 'coincident':
        args = ['Coincident', *points[c['p']], *points[c['q']]]
    elif kind == 'point_on_curve':
        args = ['PointOnObject', *points[c['p']], c['c']]
    elif kind == 'midpoint':
        args = ['Symmetric', c['l'], 1, c['l'], 2, *points[c['p']]]
    elif kind in ('radius', 'diameter'):
        args = [kind.title(), c['c'], c['value']]
    elif kind == 'length':
        args = ['Distance', c['l'], c['value']]
    elif kind in ('distance', 'distance_x', 'distance_y'):
        args = [{'distance':'Distance','distance_x':'DistanceX','distance_y':'DistanceY'}[kind], *points[c['p']], *points[c['q']], c['value']]
    elif kind == 'angle':
        args = ['Angle', c['a'], c['b'], c['value'] + (math.pi if c.get('flip') else 0)]
    elif kind == 'line_angle':
        args = ['Angle', c['l'], c['value']]
    elif kind == 'fix':
        geo, pos = points[c['p']]
        location = obj.getPoint(geo, pos)
        obj.addConstraint(Sketcher.Constraint('DistanceX', geo, pos, location.x))
        obj.addConstraint(Sketcher.Constraint('DistanceY', geo, pos, location.y))
        return
    elif kind == 'symmetric':
        args = ['Symmetric', *points[c['p']], *points[c['q']], c['l']]
    elif kind == 'concentric':
        args = ['Coincident', c['a'], 3, c['b'], 3]
    elif kind == 'collinear':
        obj.addConstraint(Sketcher.Constraint('Parallel', c['a'], c['b']))
        args = ['PointOnObject', c['a'], 1, c['b']]
    elif kind == 'point_line_distance':
        args = ['Distance', *points[c['p']], c['l'], c['value']]
    else:
        raise RuntimeError('Constraint is not supported: ' + kind)
    index = obj.addConstraint(Sketcher.Constraint(*args))
    if c.get('driven') and 'value' in c:
        obj.setDriving(index, False)

def profile(row, body, objects):
    source = objects[row['sketch']]
    selection = row.get('profiles', {'type':'all'})
    offset = quantity(row.get('extent', {}).get('start_offset', '0 mm'))
    if selection['type'] == 'all' and offset == 0:
        return source
    if selection['type'] == 'all':
        ids = None
    elif selection['type'] == 'curves':
        ids = set(curve for loop in selection['loops'] for curve in loop)
    elif selection['type'] == 'whole_boundaries':
        ids = set(curve for region in selection['profiles'] for loop in [region['outer']] + region['holes'] for curve in loop)
    else:
        raise RuntimeError('Profile selection is not supported yet')
    source_data = objects[('data', row['sketch'])]['sketch']['curves']
    selected = body.newObject('Sketcher::SketchObject', 'SelectedProfile')
    placement = source.Placement
    placement.Base += placement.Rotation.multVec(App.Vector(0,0,offset))
    selected.Placement = placement
    for i, curve in enumerate(source_data):
        if (ids is None and not curve.get('construction')) or (ids is not None and curve['id'] in ids):
            selected.addGeometry(source.Geometry[i], False)
    doc.recompute()
    visible(selected, False)
    return selected

def axis(body, origin, direction):
    obj = body.newObject('Sketcher::SketchObject', 'ReferenceAxis')
    obj.Placement = App.Placement(vector(origin), App.Rotation(App.Vector(1,0,0), vector(direction)))
    obj.addGeometry(Part.LineSegment(App.Vector(), App.Vector(1,0,0)), True)
    doc.recompute()
    visible(obj, False)
    return (obj, ['H_Axis'])

def rebuild(row, body, objects, names, tip):
    kind = row['type']
    op = row.get('operation', 'join')
    if kind == 'sketch':
        obj = sketch(row, body)
        objects[row['id']] = obj
        objects[('data', row['id'])] = row
        names[row['name']] = obj
        visible(obj, False)
        return tip
    if kind in ('extrude', 'revolve', 'loft', 'sweep'):
        types = {'extrude': ('Pad','Pocket'), 'revolve': ('Revolution','Groove'), 'loft': ('AdditiveLoft','SubtractiveLoft'), 'sweep': ('AdditivePipe','SubtractivePipe')}
        typename = types[kind][int(op == 'cut')]
        if kind == 'loft':
            sections = [profile(s, body, objects) for s in row['sections']]
            selected = sections[0]
        else:
            selected = profile(row, body, objects)
        if kind == 'revolve':
            reference = axis(body, row['axis']['origin'], row['axis']['dir'])
        if tip:
            body.Tip = tip
        obj = body.newObject('PartDesign::' + typename, 'Feature')
        obj.Profile = selected
        if kind == 'loft':
            obj.Sections = sections[1:]
            obj.Ruled, obj.Closed = False, False
        if kind == 'extrude':
            extent = row['extent']
            obj.Type = 1 if extent.get('through_all') else (4 if 'distance2' in extent else 0)
            obj.Length = quantity(extent['distance'])
            if 'distance2' in extent:
                obj.Length2 = quantity(extent['distance2'])
            obj.Reversed = extent['direction'] == 'negative'
            obj.Midplane = extent['direction'] == 'symmetric'
        elif kind == 'revolve':
            obj.ReferenceAxis = reference
            obj.Angle = quantity(row['angle'])
            obj.Midplane = row.get('symmetric', False)
        elif kind == 'sweep':
            path = objects[row['path_sketch']]
            curves = objects[('data', row['path_sketch'])]['sketch']['curves']
            indices = {c['id']: i+1 for i,c in enumerate(c for c in curves if not c.get('construction'))}
            edges = ['Edge%d' % indices[curve] for curve in row['path']]
            obj.Spine = (path, edges)
            obj.Transition = 0
    elif kind == 'pattern':
        layout = row['pattern']
        circular = layout['type'] == 'circular'
        reference = axis(body, layout['origin'], layout['axis']) if circular else axis(body, [0,0,0], layout['dir1'])
        if tip:
            body.Tip = tip
        obj = body.newObject('PartDesign::PolarPattern' if circular else 'PartDesign::LinearPattern', 'Pattern')
        obj.Originals = [names[name] for name in row['features']]
        if circular:
            obj.Axis = reference
            obj.Angle = quantity(layout['angle'])
            obj.Occurrences = int(layout['count'])
        else:
            obj.Direction = reference
            count = int(layout['count1'])
            obj.Length = quantity(layout['spacing1']) * (count-1)
            obj.Occurrences = count
    elif kind == 'hole':
        source = objects[row['points']['sketch']]
        points = objects[('data', row['points']['sketch'])]['sketch']['points']
        centers = body.newObject('Sketcher::SketchObject', 'HoleCenters')
        centers.Placement = source.Placement
        for p in points:
            if p['id'] in row['points']['ids']:
                centers.addGeometry(Part.Circle(App.Vector(*p['pos'], 0), App.Vector(0,0,1), quantity(row['diameter'])/2), False)
        if tip:
            body.Tip = tip
        obj = body.newObject('PartDesign::Hole', 'Hole')
        obj.Profile = centers
        normal = source.Placement.Rotation.multVec(App.Vector(0,0,1))
        alignment = normal.dot(vector(row['direction']))
        if abs(alignment) < 1 - 1e-9:
            raise RuntimeError('Oblique hole direction is not supported yet')
        obj.Reversed = alignment > 0
        obj.Diameter = quantity(row['diameter'])
        if 'depth' in row:
            obj.DepthType = 'Dimension'
            obj.Depth = quantity(row['depth'])
        else:
            obj.DepthType = 'Through all'
        drilled = row['hole']['type'] == 'drilled'
        obj.DrillPoint = 'Angled' if drilled else 'Flat'
        if drilled:
            obj.DrillPointAngle = quantity(row['hole']['tip_angle'])
            if hasattr(obj, 'DrillForDepth'):
                obj.DrillForDepth = False
        visible(centers, False)
    elif kind == 'coil':
        turns, pitch = quantity(row['turns']), quantity(row['pitch'])
        if not (0 < turns <= 10000 and pitch > 0):
            raise RuntimeError('Coil exceeds the macro limit')
        radius = quantity(row['diameter']) / 2
        size = quantity(row['section_size'])
        offset = row.get('position', 0) * size / 2
        path = Part.makeHelix(pitch, pitch * turns, radius, 0, row.get('clockwise', False))
        center = App.Vector(radius + offset, 0, 0)
        if row['section'] == 'circular':
            wire = Part.Wire([Part.makeCircle(size / 2, center, App.Vector(0,1,0))])
        else:
            corners = [center + App.Vector(x * size / 2, 0, z * size / 2) for x,z in [(-1,-1),(1,-1),(1,1),(-1,1),(-1,-1)]]
            wire = Part.Wire(Part.makePolygon(corners).Edges)
        shape = Part.Wire(path.Edges).makePipeShell([wire], True, True)
        shape.rotate(App.Vector(), App.Vector(0,0,1), quantity(row.get('start_angle', '0 deg')))
        shape.translate(vector(row['base']))
        if tip and op != 'new_body':
            shape = tip.Shape.cut(shape) if op == 'cut' else tip.Shape.fuse(shape)
        obj = body.newObject('PartDesign::Feature', 'Coil')
        obj.Shape = shape
        for key, prop in [('turns','Turns'), ('pitch','Pitch'), ('diameter','Diameter'), ('section_size','SectionSize')]:
            obj.addProperty('App::PropertyFloat', prop, 'Coil')
            setattr(obj, prop, quantity(row[key]))
    elif kind in ('sphere', 'torus'):
        prefix = 'Subtractive' if op == 'cut' else 'Additive'
        obj = body.newObject('PartDesign::' + prefix + kind.title(), 'Primitive')
        obj.Placement = App.Placement(vector(row['center']), App.Rotation())
        if kind == 'sphere':
            obj.Radius = quantity(row['radius'])
        else:
            obj.Radius1, obj.Radius2 = quantity(row['major']), quantity(row['minor'])
    else:
        raise RuntimeError('Feature is not supported: ' + kind)
    obj.Label = row['name']
    doc.recompute()
    if 'Invalid' in obj.State or obj.Shape.isNull() or not obj.Shape.isValid():
        raise RuntimeError('Feature failed: ' + row['name'])
    if tip:
        visible(tip, False)
    visible(obj, True)
    body.Tip = obj
    objects[row['id']] = obj
    names[row['name']] = obj
    return obj

"#;
