use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;
use wait_timeout::ChildExt;

use crate::kernel::artifacts::ArtifactFormat;

pub const ACTUAL_RENDERER_VERSION: &str = "microsoft-office-poppler/v1";
const MAX_RENDERED_PAGES: usize = 64;
const MAX_PREVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub struct ActualVisualRender {
    pub pages: Vec<Vec<u8>>,
    pub renderer_version: &'static str,
}

pub fn render_artifact_file(
    format: ArtifactFormat,
    input_path: &Path,
) -> Result<ActualVisualRender, String> {
    let canonical = input_path
        .canonicalize()
        .map_err(|_| "artifact file is unavailable for actual rendering".to_string())?;
    let canonical = office_shell_path(&canonical);
    let temp = tempfile::tempdir()
        .map_err(|_| "artifact render workspace could not be created".to_string())?;
    let pdf = match format {
        ArtifactFormat::Pdf => canonical,
        ArtifactFormat::Word | ArtifactFormat::Excel | ArtifactFormat::PowerPoint => {
            export_office_pdf(format, &canonical, &temp)?
        }
    };
    let prefix = temp.path().join("page");
    let mut command = Command::new("pdftoppm.exe");
    command
        .arg("-png")
        .arg("-r")
        .arg("120")
        .arg("-f")
        .arg("1")
        .arg("-l")
        .arg(MAX_RENDERED_PAGES.to_string())
        .arg(&pdf)
        .arg(&prefix);
    let status = run_with_timeout(&mut command, Duration::from_secs(45), "actual PDF renderer")?;
    if !status.success() {
        return Err("actual PDF renderer failed".to_string());
    }
    let mut paths = fs::read_dir(temp.path())
        .map_err(|_| "rendered previews are unavailable".to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("png"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() || paths.len() > MAX_RENDERED_PAGES {
        return Err("actual renderer returned an invalid page count".to_string());
    }
    let mut total = 0usize;
    let mut pages = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(path).map_err(|_| "rendered preview could not be read".to_string())?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("actual renderer returned a non-PNG preview".to_string());
        }
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| "rendered preview size is invalid".to_string())?;
        if total > MAX_PREVIEW_BYTES {
            return Err("rendered previews exceed the evidence limit".to_string());
        }
        pages.push(bytes);
    }
    Ok(ActualVisualRender {
        pages,
        renderer_version: ACTUAL_RENDERER_VERSION,
    })
}

fn office_shell_path(path: &Path) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn export_office_pdf(
    format: ArtifactFormat,
    input: &Path,
    temp: &TempDir,
) -> Result<PathBuf, String> {
    let pdf = temp.path().join("office-export.pdf");
    let app = match format {
        ArtifactFormat::Word => "word",
        ArtifactFormat::Excel => "excel",
        ArtifactFormat::PowerPoint => "powerpoint",
        ArtifactFormat::Pdf => return Err("PDF does not require Office export".to_string()),
    };
    let script = r#"
$ErrorActionPreference = 'Stop'
$kind = $env:DS_ARTIFACT_RENDER_KIND
$inputPath = $env:DS_ARTIFACT_RENDER_INPUT
$outputPath = $env:DS_ARTIFACT_RENDER_OUTPUT
$office = $null; $document = $null
try {
  if ($kind -eq 'word') {
    $office = New-Object -ComObject Word.Application; $office.Visible = $false
    $document = $office.Documents.Open($inputPath)
    $document.ExportAsFixedFormat($outputPath, 17)
  } elseif ($kind -eq 'excel') {
    $office = New-Object -ComObject Excel.Application; $office.Visible = $false; $office.DisplayAlerts = $false
    $document = $office.Workbooks.Open($inputPath)
    $document.ExportAsFixedFormat(0, $outputPath)
  } elseif ($kind -eq 'powerpoint') {
    $office = New-Object -ComObject PowerPoint.Application
    $document = $office.Presentations.Open($inputPath, -1, 0, 0)
    $document.SaveAs($outputPath, 32)
  } else { throw 'unsupported Office renderer' }
} finally {
  if ($document -ne $null) { try { $document.Close() } catch {} }
  if ($office -ne $null) { try { $office.Quit() } catch {} }
  if ($document -ne $null) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($document) }
  if ($office -ne $null) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($office) }
}
"#;
    let mut command = Command::new(r"C:\Program Files\PowerShell\7\pwsh.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-Command",
            script,
        ])
        .env("DS_ARTIFACT_RENDER_KIND", app)
        .env("DS_ARTIFACT_RENDER_INPUT", input)
        .env("DS_ARTIFACT_RENDER_OUTPUT", &pdf);
    let status = run_with_timeout(
        &mut command,
        Duration::from_secs(60),
        "Microsoft Office renderer",
    )?;
    if !status.success() || !pdf.is_file() {
        return Err("Microsoft Office could not render the artifact".to_string());
    }
    Ok(pdf)
}

fn run_with_timeout(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> Result<std::process::ExitStatus, String> {
    let mut child = command
        .spawn()
        .map_err(|_| format!("{label} is unavailable"))?;
    match child
        .wait_timeout(timeout)
        .map_err(|_| format!("{label} status is unavailable"))?
    {
        Some(status) => Ok(status),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("{label} timed out"))
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::kernel::artifacts::{
        ArtifactEngine, ArtifactGenerationRequest, ArtifactInput, ArtifactTemplateRef,
    };
    use crate::kernel::office::{OfficeApp, OfficeCreateSpec, OfficeSlideSpec};
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    #[ignore = "requires installed Microsoft Office and pdftoppm"]
    fn actual_renderer_produces_reviewable_pngs_for_all_four_formats() {
        for format in [
            ArtifactFormat::Word,
            ArtifactFormat::Excel,
            ArtifactFormat::PowerPoint,
            ArtifactFormat::Pdf,
        ] {
            let extension = match format {
                ArtifactFormat::Word => "docx",
                ArtifactFormat::Excel => "xlsx",
                ArtifactFormat::PowerPoint => "pptx",
                ArtifactFormat::Pdf => "pdf",
            };
            let input = match format {
                ArtifactFormat::Word => ArtifactInput::Office {
                    spec: OfficeCreateSpec {
                        app: OfficeApp::Word,
                        path: format!("fixture.{extension}"),
                        title: "Actual render fixture".to_string(),
                        body: "Visible Word content".to_string(),
                        rows: Vec::new(),
                        slides: Vec::new(),
                    },
                },
                ArtifactFormat::Excel => ArtifactInput::Office {
                    spec: OfficeCreateSpec {
                        app: OfficeApp::Excel,
                        path: format!("fixture.{extension}"),
                        title: "Actual render fixture".to_string(),
                        body: String::new(),
                        rows: vec![vec!["Metric".to_string(), "42".to_string()]],
                        slides: Vec::new(),
                    },
                },
                ArtifactFormat::PowerPoint => ArtifactInput::Office {
                    spec: OfficeCreateSpec {
                        app: OfficeApp::PowerPoint,
                        path: format!("fixture.{extension}"),
                        title: "Actual render fixture".to_string(),
                        body: String::new(),
                        rows: Vec::new(),
                        slides: vec![OfficeSlideSpec {
                            title: "Visible slide".to_string(),
                            body: "Rendered by PowerPoint".to_string(),
                        }],
                    },
                },
                ArtifactFormat::Pdf => ArtifactInput::Pdf {
                    title: "Actual render fixture".to_string(),
                    paragraphs: vec!["Visible PDF content".to_string()],
                },
            };
            let generated = ArtifactEngine::generate(
                &ArtifactGenerationRequest {
                    request_id: Uuid::new_v4(),
                    input,
                    template: ArtifactTemplateRef {
                        template_id: "actual-render-fixture".to_string(),
                        version: 1,
                        content_hash: "a".repeat(64),
                    },
                    approved_storage_ref: format!("artifact-storage:{}", Uuid::new_v4()),
                },
                Utc::now(),
            )
            .expect("fixture generates");
            let temp = tempfile::tempdir().expect("fixture tempdir");
            let path = temp.path().join(format!("fixture.{extension}"));
            fs::write(&path, generated.bytes).expect("fixture writes");
            let rendered = render_artifact_file(format, &path).expect("actual render succeeds");
            assert!(!rendered.pages.is_empty());
            assert!(rendered
                .pages
                .iter()
                .all(|page| page.starts_with(b"\x89PNG\r\n\x1a\n")));
            if let Ok(output_dir) = std::env::var("DS_ARTIFACT_PREVIEW_OUTPUT") {
                let output_dir = PathBuf::from(output_dir);
                fs::create_dir_all(&output_dir).expect("preview output creates");
                fs::write(
                    output_dir.join(format!("{format:?}-page-1.png")),
                    &rendered.pages[0],
                )
                .expect("preview copies");
                fs::copy(
                    &path,
                    output_dir.join(format!("{format:?}-fixture.{extension}")),
                )
                .expect("artifact fixture copies");
            }
        }
    }
}
