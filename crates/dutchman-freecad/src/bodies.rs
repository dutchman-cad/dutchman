//! Parts that end as several bodies: one `PartDesign::Body` per body the history makes.
use dutchman_core::Error;

use crate::generate;

/// Writes the history of a multi-body part. The result is the bodies together, compared with
/// the original STEP; any failure restores the original geometry, as for a single body.
pub(crate) fn write(script: &mut String, name: &str, code: &str) -> Result<(), Error> {
    script.push_str(PRELUDE);
    script.push_str("fallback_label = ");
    script.push_str(&generate::quoted(name)?);
    script.push_str("\ntry:\n");
    for line in code.lines() {
        script.push_str("    ");
        script.push_str(line);
        script.push('\n');
    }
    script.push_str(FINISH);
    Ok(())
}

const FINISH: &str = r#"    doc.recompute()
    if not states:
        raise RuntimeError('No body was rebuilt')
    shapes = []
    for state in states:
        if state['tip'] is None or state['body'].Shape.isNull() or not state['body'].Shape.isValid():
            raise RuntimeError('Rebuilt body has no valid shape')
        shapes.append(state['body'].Shape)
    rebuilt = doc.addObject('Part::Feature', 'RebuiltBodies')
    rebuilt.Label = fallback_label + ' (rebuilt bodies)'
    attach(rebuilt)
    rebuilt.Shape = Part.makeCompound(shapes)
    visible(rebuilt, False)
    compare_original(rebuilt.Shape, original.Shape)
    visible(original, False)
    selected = rebuilt
except Exception as error:
    fallback = str(error)
    for state in states:
        visible(state['body'], False)
    visible(original, True)
    App.Console.PrintWarning('Rebuild failed; original STEP restored: ' + str(error) + '\n')
"#;

/// Each new-body feature opens a Body. A later feature goes where the geometry says: a pattern
/// to the body holding its features, a cut to every body it removes material from, anything
/// else to the first body it joins without leaving a separate solid. A feature that fits no
/// body fails the rebuild, which restores the original STEP.
const PRELUDE: &str = r#"
def body_state(label):
    body = doc.addObject('PartDesign::Body', 'Body')
    body.Label = label
    attach(body)
    return {'body': body, 'objects': {}, 'names': {}, 'tip': None}

def body_sketches(row, state, sketch_rows):
    wanted = [row[key] for key in ('sketch', 'path_sketch') if key in row]
    wanted += [section['sketch'] for section in row.get('sections', []) if 'sketch' in section]
    if 'points' in row:
        wanted.append(row['points']['sketch'])
    for sketch_id in wanted:
        if sketch_id not in state['objects']:
            rebuild(sketch_rows[sketch_id], state['body'], state['objects'], state['names'], state['tip'])

def body_apply(row, state, sketch_rows):
    body_sketches(row, state, sketch_rows)
    state['tip'] = rebuild(row, state['body'], state['objects'], state['names'], state['tip'])

def body_try(row, state, sketch_rows):
    body_sketches(row, state, sketch_rows)
    tip = state['tip']
    before = set(obj.Name for obj in doc.Objects)
    objects, names = dict(state['objects']), dict(state['names'])
    volume, solids = tip.Shape.Volume, len(tip.Shape.Solids)
    cuts = row['type'] == 'hole' or row.get('operation', 'join') == 'cut'
    try:
        made = rebuild(row, state['body'], state['objects'], state['names'], tip)
        if cuts:
            fits = volume - made.Shape.Volume > max(abs(volume), 1.0) * 1e-7
        else:
            fits = len(made.Shape.Solids) == solids
    except Exception:
        fits = False
    if fits:
        state['tip'] = made
        return True
    state['body'].Tip = tip
    for obj in reversed(list(doc.Objects)):
        if obj.Name not in before:
            doc.removeObject(obj.Name)
    state['objects'].clear()
    state['objects'].update(objects)
    state['names'].clear()
    state['names'].update(names)
    visible(tip, True)
    doc.recompute()
    return False

def rebuild_bodies(row, states, sketch_rows):
    kind = row['type']
    if kind == 'sketch':
        sketch_rows[row['id']] = row
        return
    operation = row.get('operation', 'join')
    if operation == 'new_body' or not states:
        label = (row.get('body_names') or [fallback_label])[0]
        states.append(body_state(label))
        body_apply(row, states[-1], sketch_rows)
        return
    if len(states) == 1:
        body_apply(row, states[0], sketch_rows)
        return
    if kind == 'pattern':
        owners = [s for s in states if all(name in s['names'] for name in row['features'])]
        if len(owners) != 1:
            raise RuntimeError('Pattern features do not belong to one body: ' + row['name'])
        body_apply(row, owners[0], sketch_rows)
        return
    if kind == 'hole' or operation == 'cut':
        if not [s for s in states if body_try(row, s, sketch_rows)]:
            raise RuntimeError('Feature removes material from no body: ' + row['name'])
        return
    for state in states:
        if body_try(row, state, sketch_rows):
            return
    raise RuntimeError('Feature joins no body: ' + row['name'])

"#;
