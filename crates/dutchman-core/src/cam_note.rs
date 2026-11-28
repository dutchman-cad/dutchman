//! Report manufacturing records that the design writer does not convert.
use crate::{Report, Severity};
use cadmpeg_ir::document::CadIr;

pub(crate) fn record(ir: &CadIr, report: &mut Report) {
    let Some(native) = ir.native.namespace("f3d") else {
        return;
    };
    let count = |name: &str| {
        native
            .arenas()
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map_or(0, |(_, rows)| rows.len())
    };
    let (setups, operations, tools) = (
        count("cam_setups"),
        count("cam_operations"),
        count("cam_tools"),
    );
    if setups == 0 && operations == 0 && tools == 0 {
        return;
    }
    report.push_coded(Severity::Dropped, "CAM", "f3d/cam-not-converted",
        format!("CAM setups, toolpaths and the design's tool library were not converted. Setups: {setups}; operations: {operations}; tools: {tools}."));
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::native::NativeRecord;
    #[test]
    fn cam_note_requires_records_and_counts_each_kind() {
        let mut ir = CadIr::empty();
        let mut report = Report::default();
        record(&ir, &mut report);
        assert!(report.losses().is_empty());
        for arena in ["cam_setups", "cam_operations", "cam_tools"] {
            let mut ir = CadIr::empty();
            ir.native
                .namespace_mut(cadmpeg_ir::codec_id!("f3d"))
                .arenas_mut()
                .insert(
                    cadmpeg_core::text::NonBlankString::new(arena).unwrap(),
                    vec![NativeRecord::new(
                        cadmpeg_ir::ids::Identity::new("f3d:cam:record#1").unwrap(),
                        Default::default(),
                    )
                    .unwrap()],
                );
            let mut report = Report::default();
            record(&ir, &mut report);
            assert_eq!(report.losses().len(), 1);
            let expected = match arena {
                "cam_setups" => "Setups: 1; operations: 0; tools: 0.",
                "cam_operations" => "Setups: 0; operations: 1; tools: 0.",
                _ => "Setups: 0; operations: 0; tools: 1.",
            };
            assert!(report.losses()[0].detail.ends_with(expected));
            assert_eq!(
                report.losses()[0].code.as_deref(),
                Some("f3d/cam-not-converted")
            );
            assert!(report.losses()[0].detail.starts_with(
                "CAM setups, toolpaths and the design's tool library were not converted."
            ));
        }
        ir.native.namespace_mut(cadmpeg_ir::codec_id!("f3d"));
        record(&ir, &mut report);
        assert!(report.losses().is_empty());
    }
}
