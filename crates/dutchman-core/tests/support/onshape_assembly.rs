//! Invented assembly records; geometry is the repository's synthetic 20 x 10 x 5 mm plate.
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

pub fn matrix(angle: f64, xyz: [f64; 3]) -> Value {
    let (s, c) = angle.sin_cos();
    json!([c, -s, 0., xyz[0], s, c, 0., xyz[1], 0., 0., 1., xyz[2], 0., 0., 0., 1.])
}
fn part(id: &str) -> Value {
    json!({"id":id,"name":id,"type":"Part","documentId":"synthetic","elementId":"studio","partId":"plate","configuration":"default"})
}
fn mate(a: &str, b: &str, kind: &str) -> Value {
    let cs = json!({"origin":[0.,0.,0.],"xAxis":[1.,0.,0.],"yAxis":[0.,1.,0.],"zAxis":[0.,0.,1.]});
    // Connect the stacked plates at their touching faces, or the ring parts at
    // the common world Z axis (each part's local x origin is 60 mm from it).
    let mut other = cs.clone();
    let mut cs = cs;
    if kind == "FASTENED" {
        cs["origin"] = json!([0., 0., 0.005]);
    } else {
        cs["origin"] = json!([-0.06, 0., 0.]);
        other["origin"] = json!([-0.06, 0., 0.]);
    }
    json!({"id":"join","featureType":"mate","suppressed":false,"featureData":{"mateType":kind,"matedEntities":[{"matedOccurrence":[a],"matedCS":cs},{"matedOccurrence":[b],"matedCS":other}]}})
}
pub fn bundle(plate_bundle: &[u8], ring: bool) -> Vec<u8> {
    let mut instances = Vec::new();
    let mut occurrences = Vec::new();
    let mut subs = Vec::new();
    if ring {
        for i in 0..8 {
            let id = format!("ring{i}");
            let angle = i as f64 * std::f64::consts::TAU / 8.;
            instances.push(part(&id));
            occurrences.push(json!({"path":[id],"transform":matrix(angle,[0.06*angle.cos(),0.06*angle.sin(),0.])}));
        }
    } else {
        instances.push(part("base"));
        instances.push(json!({"id":"holder","name":"holder","type":"Assembly","documentId":"synthetic","elementId":"nested","configuration":"default"}));
        // Stored occurrences are world frames. holder * Tz(5 mm) = upper at 10 mm.
        occurrences = vec![
            json!({"path":["base"],"transform":matrix(0.,[0.,0.,0.])}),
            json!({"path":["holder"],"transform":matrix(0.,[0.,0.,0.005])}),
            json!({"path":["holder","middle"],"transform":matrix(0.,[0.,0.,0.005])}),
            json!({"path":["holder","upper"],"transform":matrix(0.,[0.,0.,0.010])}),
        ];
        subs.push(json!({"documentId":"synthetic","elementId":"nested","configuration":"default","instances":[part("middle"),part("upper")],"features":[mate("middle","upper","FASTENED")]}));
    }
    let features = if ring {
        vec![mate("ring0", "ring1", "REVOLUTE")]
    } else {
        vec![]
    };
    let definition = json!({"rootAssembly":{"instances":instances,"occurrences":occurrences,"features":features},"subAssemblies":subs,"parts":[{"documentId":"synthetic","elementId":"studio","partId":"plate","configuration":"default","name":"plate"}]});
    let members = json!({"manifest.json":{"schema":"dutchman.onshape-bundle/1","document":{"id":"synthetic","microversion":"invented"},"root_element":"assembly","elements":[{"id":"assembly","type":"ASSEMBLY"},{"id":"studio","type":"PARTSTUDIO"}]},"assemblies/assembly/definition.json":definition,"assemblies/assembly/features.json":{"features":[]},"partstudios/studio/features.json":{"features":[]},"partstudios/studio/bodydetails/index.json":[]});
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, value) in members.as_object().unwrap() {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    }
    let mut source = zip::ZipArchive::new(Cursor::new(plate_bundle)).unwrap();
    let mut step = Vec::new();
    source
        .by_name("partstudios/studio/export.step")
        .unwrap()
        .read_to_end(&mut step)
        .unwrap();
    zip.start_file(
        "partstudios/studio/export.step",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&step).unwrap();
    zip.finish().unwrap().into_inner()
}
