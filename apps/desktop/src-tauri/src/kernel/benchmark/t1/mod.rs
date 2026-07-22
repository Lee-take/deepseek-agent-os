pub mod baseline;
#[cfg(test)]
mod c4c;
pub mod fixtures;
pub mod verifiers;

use crate::kernel::policy::{CapabilityKind, RiskLevel};

use self::fixtures::generate_fixture_set;
use super::{
    BenchmarkDoneWhenSpec, BenchmarkExpectedTerminal, BenchmarkFixtureDataClass,
    BenchmarkFixtureProvenance, BenchmarkFixtureSpec, BenchmarkTaskSpec,
    BENCHMARK_TASK_SPEC_VERSION,
};

pub const TASK_ID: &str = "t1-monthly-operations-brief";
pub const FIXTURE_SET_ID: &str = "t1-monthly-operations-brief-fixture-set-v1";
pub const FIXTURE_GENERATOR_ID: &str = "t1-synthetic-office-fixtures-v1";

pub const SOURCE_MANIFEST_VERIFIER_ID: &str = "t1.source-manifest/v1";
pub const PROVENANCE_VERIFIER_ID: &str = "t1.provenance/v1";
pub const RECONCILIATION_XLSX_VERIFIER_ID: &str = "t1.reconciliation-xlsx/v1";
pub const ONE_PAGE_PPTX_VERIFIER_ID: &str = "t1.one-page-pptx/v1";
pub const ACTUAL_RENDER_VERIFIER_ID: &str = "t1.actual-render/v1";
pub const RESULT_RECEIPT_VERIFIER_ID: &str = "t1.result-receipt/v1";

pub const RECONCILIATION_OUTPUT_PATH: &str = "outputs/t1-reconciliation.xlsx";
pub const BRIEF_OUTPUT_PATH: &str = "outputs/t1-monthly-brief.pptx";

pub fn task_spec() -> Result<BenchmarkTaskSpec, String> {
    let fixture_set = generate_fixture_set()?;
    let fixtures = fixture_set
        .manifest
        .files
        .iter()
        .map(|fixture| BenchmarkFixtureSpec {
            fixture_id: fixture.fixture_id.clone(),
            relative_path: fixture.relative_path.clone(),
            media_type: fixture.media_type.clone(),
            sha256: fixture.sha256.clone(),
            provenance: BenchmarkFixtureProvenance {
                source_kind: "synthetic_generator".to_string(),
                generator_id: fixture_set.manifest.generator_id.clone(),
                source_label: fixture.source_label.clone(),
            },
        })
        .collect();
    let done_when = vec![
        done_when(
            "source-manifest",
            "The three synthetic inputs are complete and hash-bound.",
            SOURCE_MANIFEST_VERIFIER_ID,
            "source_manifest",
        ),
        done_when(
            "fact-provenance",
            "Every critical source and derived fact is traceable and recomputable.",
            PROVENANCE_VERIFIER_ID,
            "fact_provenance",
        ),
        done_when(
            "reconciliation-xlsx",
            "The reconciliation workbook is a valid formula-backed XLSX.",
            RECONCILIATION_XLSX_VERIFIER_ID,
            "reconciliation_xlsx",
        ),
        done_when(
            "one-page-brief",
            "The monthly brief is a complete single-slide PPTX.",
            ONE_PAGE_PPTX_VERIFIER_ID,
            "one_page_pptx",
        ),
        done_when(
            "actual-render",
            "Deterministic actual-render receipts bind nonblank unclipped previews.",
            ACTUAL_RENDER_VERIFIER_ID,
            "actual_render_receipt",
        ),
        done_when(
            "result-receipt",
            "The secret-safe result receipt binds the run, outputs, facts, and evidence.",
            RESULT_RECEIPT_VERIFIER_ID,
            "result_receipt",
        ),
    ];
    let spec = BenchmarkTaskSpec {
        version: BENCHMARK_TASK_SPEC_VERSION.to_string(),
        task_id: TASK_ID.to_string(),
        task_revision: 1,
        title: "T1 monthly operations brief".to_string(),
        prompt: "Summarize the specified synthetic Excel, Word, and PDF inputs into a reconciliation workbook and a one-page monthly operations brief, flag anomalies, and save both outputs.".to_string(),
        fixture_set_id: FIXTURE_SET_ID.to_string(),
        fixture_data_class: BenchmarkFixtureDataClass::Synthetic,
        fixtures,
        done_when,
        allowed_capabilities: vec![CapabilityKind::FileRead, CapabilityKind::FileWrite],
        expected_risk: RiskLevel::High,
        authorization_budget: 1,
        expected_terminal: BenchmarkExpectedTerminal::VerifiedCompletion,
    };
    spec.validate()?;
    Ok(spec)
}

fn done_when(
    done_when_id: &str,
    description: &str,
    verifier_id: &str,
    evidence_kind: &str,
) -> BenchmarkDoneWhenSpec {
    BenchmarkDoneWhenSpec {
        done_when_id: done_when_id.to_string(),
        description: description.to_string(),
        verifier_id: verifier_id.to_string(),
        required_evidence_kinds: vec![evidence_kind.to_string()],
    }
}
