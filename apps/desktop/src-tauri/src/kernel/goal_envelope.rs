use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GOAL_ENVELOPE_PROPOSAL_VERSION: &str = "ds-agent.goal-envelope-proposal/v1";

const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_ID_BYTES: usize = 96;
const MAX_USER_GOAL_BYTES: usize = 4 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024;
const MAX_LIST_ITEMS: usize = 32;
const MAX_VERIFIERS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoalEnvelopeProposalError {
    JsonTooLarge,
    InvalidJson,
    UnsupportedVersion,
    InvalidText,
    InvalidIdentifier,
    CollectionOutOfBounds,
    DuplicateValue,
    UnknownDoneWhenBinding,
    MissingVerifierBinding,
    SecretLikeContent,
}

impl fmt::Display for GoalEnvelopeProposalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::JsonTooLarge => "goal envelope proposal json is too large",
            Self::InvalidJson => "goal envelope proposal json is invalid",
            Self::UnsupportedVersion => "goal envelope proposal version is unsupported",
            Self::InvalidText => "goal envelope proposal contains invalid text",
            Self::InvalidIdentifier => "goal envelope proposal contains an invalid identifier",
            Self::CollectionOutOfBounds => {
                "goal envelope proposal collection is outside its bounds"
            }
            Self::DuplicateValue => "goal envelope proposal contains a duplicate value",
            Self::UnknownDoneWhenBinding => {
                "goal envelope proposal verifier references an unknown done_when"
            }
            Self::MissingVerifierBinding => {
                "goal envelope proposal done_when has no verifier proposal"
            }
            Self::SecretLikeContent => "goal envelope proposal contains secret-like content",
        })
    }
}

impl std::error::Error for GoalEnvelopeProposalError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalDoneWhenProposal {
    pub done_when_id: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalRequiredArtifactProposal {
    pub artifact_id: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalVerifierProposal {
    pub verifier_id: String,
    pub done_when_id: String,
    pub description: String,
    pub evidence_kind: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalExternalTargetProposal {
    pub target_id: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GoalEnvelopeProposal {
    pub version: String,
    pub user_goal: String,
    pub assumptions: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<GoalDoneWhenProposal>,
    pub required_artifacts: Vec<GoalRequiredArtifactProposal>,
    pub verifiers: Vec<GoalVerifierProposal>,
    pub proposed_capabilities: Vec<String>,
    pub external_targets: Vec<GoalExternalTargetProposal>,
    pub stop_conditions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalEnvelopeProposalWire {
    version: String,
    user_goal: String,
    assumptions: Vec<String>,
    constraints: Vec<String>,
    done_when: Vec<GoalDoneWhenProposal>,
    required_artifacts: Vec<GoalRequiredArtifactProposal>,
    verifiers: Vec<GoalVerifierProposal>,
    proposed_capabilities: Vec<String>,
    external_targets: Vec<GoalExternalTargetProposal>,
    stop_conditions: Vec<String>,
}

impl From<GoalEnvelopeProposalWire> for GoalEnvelopeProposal {
    fn from(wire: GoalEnvelopeProposalWire) -> Self {
        Self {
            version: wire.version,
            user_goal: wire.user_goal,
            assumptions: wire.assumptions,
            constraints: wire.constraints,
            done_when: wire.done_when,
            required_artifacts: wire.required_artifacts,
            verifiers: wire.verifiers,
            proposed_capabilities: wire.proposed_capabilities,
            external_targets: wire.external_targets,
            stop_conditions: wire.stop_conditions,
        }
    }
}

impl<'de> Deserialize<'de> for GoalEnvelopeProposal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let proposal = Self::from(GoalEnvelopeProposalWire::deserialize(deserializer)?);
        proposal.validate().map_err(serde::de::Error::custom)?;
        Ok(proposal)
    }
}

impl GoalEnvelopeProposal {
    pub fn parse_json(json: &str) -> Result<Self, GoalEnvelopeProposalError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(GoalEnvelopeProposalError::JsonTooLarge);
        }
        let wire: GoalEnvelopeProposalWire =
            serde_json::from_str(json).map_err(|_| GoalEnvelopeProposalError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn parse_value(value: Value) -> Result<Self, GoalEnvelopeProposalError> {
        let encoded =
            serde_json::to_vec(&value).map_err(|_| GoalEnvelopeProposalError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(GoalEnvelopeProposalError::JsonTooLarge);
        }
        let wire: GoalEnvelopeProposalWire =
            serde_json::from_value(value).map_err(|_| GoalEnvelopeProposalError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn to_json(&self) -> Result<String, GoalEnvelopeProposalError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| GoalEnvelopeProposalError::InvalidJson)
    }

    pub fn validate(&self) -> Result<(), GoalEnvelopeProposalError> {
        if self.version != GOAL_ENVELOPE_PROPOSAL_VERSION {
            return Err(GoalEnvelopeProposalError::UnsupportedVersion);
        }
        validate_text(&self.user_goal, MAX_USER_GOAL_BYTES)?;
        validate_text_list(&self.assumptions, MAX_LIST_ITEMS)?;
        validate_text_list(&self.constraints, MAX_LIST_ITEMS)?;
        validate_text_list(&self.stop_conditions, MAX_LIST_ITEMS)?;

        if self.done_when.is_empty() || self.done_when.len() > MAX_LIST_ITEMS {
            return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
        }
        let mut done_when_ids = BTreeSet::new();
        for condition in &self.done_when {
            validate_id(&condition.done_when_id)?;
            validate_text(&condition.description, MAX_TEXT_BYTES)?;
            if !done_when_ids.insert(condition.done_when_id.as_str()) {
                return Err(GoalEnvelopeProposalError::DuplicateValue);
            }
        }

        if self.required_artifacts.len() > MAX_LIST_ITEMS {
            return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
        }
        let mut artifact_ids = BTreeSet::new();
        for artifact in &self.required_artifacts {
            validate_id(&artifact.artifact_id)?;
            validate_text(&artifact.description, MAX_TEXT_BYTES)?;
            if !artifact_ids.insert(artifact.artifact_id.as_str()) {
                return Err(GoalEnvelopeProposalError::DuplicateValue);
            }
        }

        if self.verifiers.is_empty() || self.verifiers.len() > MAX_VERIFIERS {
            return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
        }
        let mut verifier_ids = BTreeSet::new();
        let mut bound_done_when = BTreeSet::new();
        for verifier in &self.verifiers {
            validate_id(&verifier.verifier_id)?;
            validate_id(&verifier.done_when_id)?;
            validate_text(&verifier.description, MAX_TEXT_BYTES)?;
            validate_id(&verifier.evidence_kind)?;
            if !verifier_ids.insert(verifier.verifier_id.as_str()) {
                return Err(GoalEnvelopeProposalError::DuplicateValue);
            }
            if !done_when_ids.contains(verifier.done_when_id.as_str()) {
                return Err(GoalEnvelopeProposalError::UnknownDoneWhenBinding);
            }
            bound_done_when.insert(verifier.done_when_id.as_str());
        }
        if done_when_ids
            .iter()
            .any(|done_when_id| !bound_done_when.contains(done_when_id))
        {
            return Err(GoalEnvelopeProposalError::MissingVerifierBinding);
        }

        validate_id_list(&self.proposed_capabilities, MAX_LIST_ITEMS)?;

        if self.external_targets.len() > MAX_LIST_ITEMS {
            return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
        }
        let mut target_ids = BTreeSet::new();
        for target in &self.external_targets {
            validate_id(&target.target_id)?;
            validate_text(&target.description, MAX_TEXT_BYTES)?;
            if !target_ids.insert(target.target_id.as_str()) {
                return Err(GoalEnvelopeProposalError::DuplicateValue);
            }
        }

        let encoded =
            serde_json::to_vec(self).map_err(|_| GoalEnvelopeProposalError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(GoalEnvelopeProposalError::JsonTooLarge);
        }

        Ok(())
    }
}

fn validate_text_list(
    values: &[String],
    max_items: usize,
) -> Result<(), GoalEnvelopeProposalError> {
    if values.len() > max_items {
        return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
    }
    let mut unique = BTreeSet::new();
    for value in values {
        validate_text(value, MAX_TEXT_BYTES)?;
        if !unique.insert(value.as_str()) {
            return Err(GoalEnvelopeProposalError::DuplicateValue);
        }
    }
    Ok(())
}

fn validate_id_list(values: &[String], max_items: usize) -> Result<(), GoalEnvelopeProposalError> {
    if values.len() > max_items {
        return Err(GoalEnvelopeProposalError::CollectionOutOfBounds);
    }
    let mut unique = BTreeSet::new();
    for value in values {
        validate_id(value)?;
        if !unique.insert(value.as_str()) {
            return Err(GoalEnvelopeProposalError::DuplicateValue);
        }
    }
    Ok(())
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), GoalEnvelopeProposalError> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > max_bytes
        || value.chars().any(char::is_control)
    {
        return Err(GoalEnvelopeProposalError::InvalidText);
    }
    if contains_secret_like_content(value) {
        return Err(GoalEnvelopeProposalError::SecretLikeContent);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), GoalEnvelopeProposalError> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(GoalEnvelopeProposalError::InvalidIdentifier);
    }
    if contains_secret_like_content(value) {
        return Err(GoalEnvelopeProposalError::SecretLikeContent);
    }
    Ok(())
}

fn contains_secret_like_content(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    contains_token_after(&lower, "bearer ", 12)
        || contains_token_after(&lower, "api_key=", 12)
        || contains_token_after(&lower, "api_key:", 12)
        || contains_token_after(&lower, "api-key=", 12)
        || contains_token_after(&lower, "api-key:", 12)
        || contains_token_after(&lower, "apikey=", 12)
        || contains_token_after(&lower, "apikey:", 12)
        || contains_token_after(&lower, "password=", 12)
        || contains_token_after(&lower, "password:", 12)
        || contains_token_after(&lower, "secret=", 12)
        || contains_token_after(&lower, "secret:", 12)
        || contains_token_after(&lower, "token=", 12)
        || contains_token_after(&lower, "token:", 12)
        || lower.match_indices("sk-").any(|(index, _)| {
            lower[index + 3..]
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
                .count()
                >= 12
        })
}

fn contains_token_after(value: &str, marker: &str, minimum_length: usize) -> bool {
    value.match_indices(marker).any(|(index, _)| {
        value[index + marker.len()..]
            .trim_start_matches([' ', '\'', '"'])
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.'))
            .count()
            >= minimum_length
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_value() -> Value {
        serde_json::json!({
            "version": GOAL_ENVELOPE_PROPOSAL_VERSION,
            "user_goal": "Create a verified monthly operating brief.",
            "assumptions": ["The supplied source set is complete."],
            "constraints": ["Keep all proposed outputs inside the selected workspace."],
            "done_when": [
                {
                    "done_when_id": "reconciliation-ready",
                    "description": "The proposed reconciliation workbook is complete."
                },
                {
                    "done_when_id": "brief-ready",
                    "description": "The proposed one-page brief is complete."
                }
            ],
            "required_artifacts": [
                {
                    "artifact_id": "reconciliation-workbook",
                    "description": "A proposed reconciliation workbook."
                },
                {
                    "artifact_id": "operating-brief",
                    "description": "A proposed one-page presentation."
                }
            ],
            "verifiers": [
                {
                    "verifier_id": "workbook-verifier-v1",
                    "done_when_id": "reconciliation-ready",
                    "description": "Verify formulas and source totals.",
                    "evidence_kind": "workbook-reconciliation"
                },
                {
                    "verifier_id": "brief-verifier-v1",
                    "done_when_id": "brief-ready",
                    "description": "Verify the rendered slide has no overflow.",
                    "evidence_kind": "rendered-presentation"
                }
            ],
            "proposed_capabilities": ["file.read", "office.workbook.propose"],
            "external_targets": [
                {
                    "target_id": "selected-workspace",
                    "description": "The workspace selected by the user; DS Agent must validate it locally."
                }
            ],
            "stop_conditions": ["Stop if a required source is missing."]
        })
    }

    #[test]
    fn parses_and_round_trips_the_versioned_contract() {
        let proposal = GoalEnvelopeProposal::parse_value(valid_value()).expect("proposal parses");
        assert_eq!(proposal.version, GOAL_ENVELOPE_PROPOSAL_VERSION);
        assert_eq!(proposal.done_when.len(), 2);
        assert_eq!(proposal.verifiers.len(), 2);
        assert_eq!(proposal.proposed_capabilities[0], "file.read");

        let encoded = proposal.to_json().expect("proposal serializes");
        assert_eq!(
            GoalEnvelopeProposal::parse_json(&encoded).expect("round trip parses"),
            proposal
        );
    }

    #[test]
    fn rejects_missing_and_unknown_fields() {
        let mut missing = valid_value();
        missing.as_object_mut().unwrap().remove("constraints");
        assert_eq!(
            GoalEnvelopeProposal::parse_value(missing),
            Err(GoalEnvelopeProposalError::InvalidJson)
        );

        let mut unknown = valid_value();
        unknown["approval_granted"] = serde_json::json!(true);
        assert_eq!(
            GoalEnvelopeProposal::parse_value(unknown),
            Err(GoalEnvelopeProposalError::InvalidJson)
        );
    }

    #[test]
    fn rejects_unsupported_versions_without_fallback() {
        let mut value = valid_value();
        value["version"] = serde_json::json!("ds-agent.goal-envelope-proposal/v2");
        assert_eq!(
            GoalEnvelopeProposal::parse_value(value),
            Err(GoalEnvelopeProposalError::UnsupportedVersion)
        );
    }

    #[test]
    fn rejects_secret_like_content_with_a_stable_error() {
        let mut value = valid_value();
        let secret_like_value = ["sk", "1234567890abcdef"].join("-");
        value["user_goal"] = serde_json::json!(format!("Use {secret_like_value} to run the task."));
        let error = GoalEnvelopeProposal::parse_value(value).unwrap_err();
        assert_eq!(error, GoalEnvelopeProposalError::SecretLikeContent);
        assert_eq!(
            error.to_string(),
            "goal envelope proposal contains secret-like content"
        );
        assert!(!error.to_string().contains("1234567890abcdef"));
    }

    #[test]
    fn rejects_unbounded_and_duplicate_collections() {
        let mut unbounded = valid_value();
        unbounded["assumptions"] = serde_json::json!(vec!["bounded"; MAX_LIST_ITEMS + 1]);
        assert_eq!(
            GoalEnvelopeProposal::parse_value(unbounded),
            Err(GoalEnvelopeProposalError::CollectionOutOfBounds)
        );

        let mut duplicate = valid_value();
        duplicate["proposed_capabilities"] = serde_json::json!(["file.read", "file.read"]);
        assert_eq!(
            GoalEnvelopeProposal::parse_value(duplicate),
            Err(GoalEnvelopeProposalError::DuplicateValue)
        );
    }

    #[test]
    fn rejects_an_oversized_aggregate_after_direct_dto_mutation() {
        let mut proposal =
            GoalEnvelopeProposal::parse_value(valid_value()).expect("proposal parses");
        proposal.assumptions = (0..MAX_LIST_ITEMS)
            .map(|index| format!("{index:02}-{}", "x".repeat(MAX_TEXT_BYTES - 3)))
            .collect();

        assert_eq!(
            proposal.validate(),
            Err(GoalEnvelopeProposalError::JsonTooLarge)
        );
        assert_eq!(
            proposal.to_json(),
            Err(GoalEnvelopeProposalError::JsonTooLarge)
        );
    }

    #[test]
    fn rejects_unknown_and_missing_verifier_bindings() {
        let mut unknown = valid_value();
        unknown["verifiers"][0]["done_when_id"] = serde_json::json!("not-declared");
        assert_eq!(
            GoalEnvelopeProposal::parse_value(unknown),
            Err(GoalEnvelopeProposalError::UnknownDoneWhenBinding)
        );

        let mut missing = valid_value();
        missing["verifiers"].as_array_mut().unwrap().pop();
        assert_eq!(
            GoalEnvelopeProposal::parse_value(missing),
            Err(GoalEnvelopeProposalError::MissingVerifierBinding)
        );
    }

    #[test]
    fn rejects_authority_fields_inside_nested_proposals() {
        let mut value = valid_value();
        value["external_targets"][0]["trusted_path"] = serde_json::json!(true);
        assert_eq!(
            GoalEnvelopeProposal::parse_value(value),
            Err(GoalEnvelopeProposalError::InvalidJson)
        );
    }

    #[test]
    fn keeps_unknown_capability_names_as_non_authoritative_proposals() {
        let mut value = valid_value();
        value["proposed_capabilities"] =
            serde_json::json!(["future.connector.read", "future.desktop.observe"]);
        let proposal = GoalEnvelopeProposal::parse_value(value).expect("proposal parses");
        assert_eq!(
            proposal.proposed_capabilities,
            vec!["future.connector.read", "future.desktop.observe"]
        );
    }
}
