//! The version of the program that saved the input file, read from numbers the file states.

use std::path::Path;

use cadmpeg_ir::native::Native;

use crate::Report;

/// Inventor's `RSeDb` database record stores the version of the application that saved it
/// as three numbers. A file may hold several databases; the newest saving application wins,
/// but only when every record is valid: one malformed record means the file's version is not
/// known, rather than quietly reporting an older database.
/// Returns `major.minor.revision`, or `None` when the file states no such record.
pub(crate) fn of(native: &Native) -> Option<String> {
    let records = native.namespace("inventor")?.arenas().get("databases")?;
    let versions = records
        .iter()
        .map(|record| {
            let saved = record.field("saved_by")?;
            let part = |name: &str| {
                saved
                    .get(name)?
                    .as_u64()
                    .filter(|n| *n <= u64::from(u8::MAX))
            };
            Some((part("major")?, part("minor")?, part("revision")?))
        })
        .collect::<Option<Vec<_>>>()?;
    versions
        .into_iter()
        .max()
        .map(|(major, minor, revision)| format!("{major}.{minor}.{revision}"))
}

/// Records the saving version of a decoded file on the report, but only for the input itself:
/// a part or component found beside it was saved by its own program version, which says
/// nothing about the file that was submitted.
pub(crate) fn record(report: &mut Report, decoded: &Path, input: &Path, native: &Native) {
    let is_input = decoded == input
        || matches!((decoded.canonicalize(), input.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if is_input {
        if let Some(version) = of(native) {
            report.record_source_version(version);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native(databases: serde_json::Value) -> Native {
        let mut wire = serde_json::to_value(cadmpeg_ir::CadIr::empty()).unwrap();
        wire["native"]["inventor"] = json!({ "databases": databases });
        cadmpeg_ir::CadIr::from_json(&wire.to_string())
            .unwrap()
            .native
    }

    fn database(band: u32, major: u64) -> serde_json::Value {
        json!({
            "id": format!("inventor:rse:database#v{band}"), "band": band,
            "database_id": "00", "schema": 31,
            "created_by": {"major": 20, "minor": 0, "revision": 0, "state": "00"},
            "created_filetime": 1,
            "saved_by": {"major": major, "minor": 2, "revision": 1, "state": "00"},
            "saved_filetime": 2, "note": "synthetic",
        })
    }

    #[test]
    fn a_saved_by_record_gives_its_three_numbers() {
        assert_eq!(
            of(&native(json!([database(1, 25)]))).as_deref(),
            Some("25.2.1")
        );
    }

    #[test]
    fn the_newest_saving_application_of_several_databases_wins() {
        let databases = json!([database(1, 22), database(2, 26), database(3, 24)]);
        assert_eq!(of(&native(databases)).as_deref(), Some("26.2.1"));
    }

    #[test]
    fn one_malformed_record_makes_the_version_unknown() {
        let mut absent = database(2, 30);
        absent["saved_by"] = json!(null);
        let mut text = database(3, 30);
        text["saved_by"]["major"] = json!("30");
        for bad in [absent, text] {
            assert_eq!(of(&native(json!([database(1, 25), bad]))), None);
        }
        assert_eq!(
            of(&native(json!([database(1, 25), database(2, 26)]))).as_deref(),
            Some("26.2.1")
        );
    }

    #[test]
    fn only_the_input_file_records_its_version() {
        let input = Path::new("/synthetic/input.iam");
        let component = Path::new("/synthetic/component.ipt");
        let mut report = Report::default();
        // The component decodes first and was saved by a newer release than the input.
        record(
            &mut report,
            component,
            input,
            &native(json!([database(1, 28)])),
        );
        assert_eq!(report.source_version(), None);
        record(&mut report, input, input, &native(json!([database(1, 25)])));
        record(
            &mut report,
            component,
            input,
            &native(json!([database(1, 30)])),
        );
        assert_eq!(report.source_version(), Some("25.2.1"));
    }

    #[test]
    fn no_database_record_means_no_version() {
        assert_eq!(of(&native(json!([]))), None);
        assert_eq!(of(&cadmpeg_ir::CadIr::empty().native), None);
    }

    #[test]
    fn out_of_range_or_missing_numbers_are_not_a_version() {
        let mut huge = database(1, 25);
        huge["saved_by"]["major"] = json!(4_000_000_000u64);
        let mut text = database(2, 25);
        text["saved_by"]["minor"] = json!("2");
        let mut absent = database(3, 25);
        absent["saved_by"] = json!(null);
        assert_eq!(of(&native(json!([huge, text, absent]))), None);
    }
}
