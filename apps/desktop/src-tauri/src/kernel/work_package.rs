use crate::kernel::models::{FoundationState, TaskRecord};

pub const WORK_PACKAGE_VERSION: &str = "deepseek-agent-os.work-package.v1";

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkPackageError {
    #[error("invalid work package json: {0}")]
    InvalidJson(String),

    #[error("unsupported work package version: {0}")]
    UnsupportedVersion(String),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackage {
    pub version: String,
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub foundation_state: FoundationState,
    pub task_records: Vec<TaskRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

pub fn export_work_package(
    foundation_state: FoundationState,
    task_records: Vec<TaskRecord>,
) -> WorkPackage {
    WorkPackage {
        version: WORK_PACKAGE_VERSION.to_string(),
        exported_at: chrono::Utc::now(),
        foundation_state,
        task_records,
    }
}

pub fn parse_work_package_json(package_json: &str) -> Result<WorkPackage, WorkPackageError> {
    let package = serde_json::from_str::<WorkPackage>(package_json)
        .map_err(|error| WorkPackageError::InvalidJson(error.to_string()))?;

    if package.version != WORK_PACKAGE_VERSION {
        return Err(WorkPackageError::UnsupportedVersion(package.version));
    }

    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::{export_work_package, parse_work_package_json, WorkPackageError};
    use crate::kernel::models::{FoundationState, TaskRecord};

    #[test]
    fn exports_versioned_work_package_with_task_records() {
        let record = TaskRecord::new(
            "Prepare sales briefing".to_string(),
            "Summarize inbox and drive evidence for Monday review.".to_string(),
        )
        .expect("record is valid");

        let package = export_work_package(FoundationState::default(), vec![record.clone()]);

        assert_eq!(package.version, "deepseek-agent-os.work-package.v1");
        assert!(package.exported_at <= chrono::Utc::now());
        assert_eq!(package.foundation_state, FoundationState::default());
        assert_eq!(package.task_records, vec![record]);
    }

    #[test]
    fn rejects_work_package_with_unknown_version() {
        let mut package = export_work_package(FoundationState::default(), Vec::new());
        package.version = "deepseek-agent-os.work-package.v0".to_string();
        let package_json = serde_json::to_string(&package).expect("package serializes");

        let error = parse_work_package_json(&package_json).expect_err("version should fail");

        assert_eq!(
            error,
            WorkPackageError::UnsupportedVersion("deepseek-agent-os.work-package.v0".to_string())
        );
    }
}
