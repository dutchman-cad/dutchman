//! Page output planning and publication shared by the CLI and JSON reports.

use std::collections::BTreeSet;
use std::path::Path;

use dutchman_core::{Error, Format, Report};
use serde_json::Value;

use crate::json;

/// Converts all selected pages, planning every filename and encoding before writing files.
pub fn convert(input: &Path, output: &Path, page: Option<&str>, replace: bool) -> Value {
    match run(input, output, page, replace, None) {
        Ok(value) => value,
        Err(error) => json::error(json::CONVERT, json::input(input), &error),
    }
}

/// CLI drawing conversion with the same live diagnostic log as other output formats.
pub fn convert_with_diagnostics(
    input: &Path,
    output: &Path,
    page: Option<&str>,
    replace: bool,
    options: &crate::ConvertOptions,
) -> Value {
    crate::diagnostics::run(input, options, &|_| {}, |callback, _| {
        let progress = dutchman_core::Progress::new(callback);
        match run(input, output, page, replace, Some(&progress)) {
            Ok(value) => value,
            Err(error) => json::error(json::CONVERT, json::input(input), &error),
        }
    })
}

fn run(
    input: &Path,
    output: &Path,
    page: Option<&str>,
    replace: bool,
    progress: Option<&dutchman_core::Progress<'_>>,
) -> Result<Value, Error> {
    let stage = |stage| {
        if let Some(progress) = progress {
            progress.stage(stage);
        }
    };
    stage(dutchman_core::Stage::Input);
    if Format::from_path(input) != Some(Format::FreeCadDocument) {
        return Err(Error::Encode(
            "DXF drawing output is supported only for FreeCAD TechDraw pages; choose a .FCStd file"
                .into(),
        ));
    }
    let mut report = Report::default();
    stage(dutchman_core::Stage::Decode);
    let model = dutchman_core::read_drawings(input, &mut report)?;
    stage(dutchman_core::Stage::Map);
    let pages = dutchman_dxf::pages(&model, page, &mut report)?;
    let directory = output.is_dir();
    if !directory
        && !output
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("dxf"))
    {
        return Err(Error::WrongOutputExtension {
            path: output.into(),
            expected: "dxf",
        });
    }
    let stem = if directory {
        input.file_stem()
    } else {
        output.file_stem()
    }
    .and_then(|s| s.to_str())
    .unwrap_or("Drawing");
    let folder = if directory {
        output
    } else {
        output.parent().unwrap_or(Path::new("."))
    };
    let exact = !directory && page.is_some();
    let mut paths = BTreeSet::new();
    let mut planned = Vec::new();
    for page in pages {
        let base = dutchman_dxf::safe_name(&page.name);
        let mut path = if exact {
            output.to_path_buf()
        } else {
            folder.join(format!("{stem}-{base}.dxf"))
        };
        let mut suffix = 2_u32;
        while !paths.insert(path.to_string_lossy().to_lowercase()) {
            path = folder.join(format!("{stem}-{base}-{suffix}.dxf"));
            suffix = suffix.saturating_add(1);
        }
        if input == path
            || std::fs::canonicalize(input)
                .ok()
                .zip(std::fs::canonicalize(&path).ok())
                .is_some_and(|(a, b)| a == b)
        {
            return Err(Error::Encode(
                "DXF output would replace the input; choose another output path".into(),
            ));
        }
        if !replace
            && path.try_exists().map_err(|source| Error::Io {
                path: path.clone(),
                source,
            })?
        {
            return Err(Error::OutputExists { path });
        }
        planned.push((path, page));
    }
    stage(dutchman_core::Stage::Write);
    let mut results = Vec::new();
    for (path, page) in planned {
        use std::io::Write;
        let io = |source| Error::Io {
            path: path.clone(),
            source,
        };
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(replace)
            .truncate(replace)
            .create_new(!replace)
            .open(&path)
            .map_err(io)?;
        file.write_all(&page.bytes).map_err(io)?;
        results.push(json::drawing_page(
            &page.name,
            &path,
            &page.entities_per_layer,
            page.dimension_representation,
            &page.report,
        ));
        report.append_prefixed(page.report, &page.name);
    }
    Ok(json::drawing_success(input, &results, &report))
}
