use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDefinitionStatus {
    Enabled,
    Paused,
    Deleted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationSchedule {
    Once {
        run_at: DateTime<Utc>,
    },
    Daily {
        hour: u32,
        minute: u32,
    },
    Weekly {
        weekday: u32,
        hour: u32,
        minute: u32,
    },
    Monthly {
        day: u32,
        hour: u32,
        minute: u32,
    },
    RestrictedCron {
        weekdays: Vec<u32>,
        hour: u32,
        minute: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissedRunPolicy {
    Skip,
    RunOnce,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AutomationDefinition {
    pub id: Uuid,
    #[serde(default)]
    pub revision: u64,
    pub goal: String,
    pub timezone: String,
    pub schedule: AutomationSchedule,
    pub status: AutomationDefinitionStatus,
    pub missed_run_policy: MissedRunPolicy,
    pub retry_limit: u32,
    pub missed_after_seconds: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AutomationDefinition {
    pub fn once(goal: String, timezone: String, run_at: DateTime<Utc>) -> Result<Self, String> {
        let goal = required_text(goal, "automation goal")?;
        let timezone = required_text(timezone, "automation timezone")?;
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            revision: 0,
            goal,
            timezone,
            schedule: AutomationSchedule::Once { run_at },
            status: AutomationDefinitionStatus::Enabled,
            missed_run_policy: MissedRunPolicy::RunOnce,
            retry_limit: 0,
            missed_after_seconds: 300,
            created_at: now,
            updated_at: now,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Queued,
    Running,
    WaitingReview,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AutomationRun {
    pub id: Uuid,
    pub definition_id: Uuid,
    #[serde(default)]
    pub definition_revision: u64,
    pub trigger_window_key: String,
    pub scheduled_for: DateTime<Utc>,
    pub status: AutomationRunStatus,
    pub attempt: u32,
    pub agent_run_id: Option<Uuid>,
    pub review_queue_item_id: Option<Uuid>,
    pub last_error: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AutomationCheckpoint {
    pub automation_run_id: Uuid,
    pub dedup_key: String,
    pub tool_invocation_id: Option<Uuid>,
    pub evidence_ref: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewQueueItemStatus {
    PendingReview,
    PendingApproval,
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewQueueItem {
    pub id: Uuid,
    pub automation_run_id: Uuid,
    pub agent_run_id: Option<Uuid>,
    pub tool_invocation_id: Option<Uuid>,
    pub status: ReviewQueueItemStatus,
    #[serde(default)]
    pub preview_fingerprint: Option<String>,
    #[serde(default)]
    pub revision: u32,
    pub title: String,
    pub evidence_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ReviewQueueItem {
    pub fn edit(
        &mut self,
        title: String,
        preview_fingerprint: Option<String>,
        changed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if !matches!(
            self.status,
            ReviewQueueItemStatus::PendingReview | ReviewQueueItemStatus::PendingApproval
        ) {
            return Err("resolved review item cannot be edited".to_string());
        }
        let title = required_text(title, "review item title")?;
        self.title = title;
        self.preview_fingerprint = preview_fingerprint;
        self.tool_invocation_id = None;
        self.status = ReviewQueueItemStatus::PendingReview;
        self.revision = self.revision.saturating_add(1);
        self.updated_at = changed_at;
        Ok(())
    }

    pub fn request_approval(
        &mut self,
        tool_invocation_id: Uuid,
        exact_fingerprint: String,
        changed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != ReviewQueueItemStatus::PendingReview {
            return Err("review item is not ready to request approval".to_string());
        }
        let exact_fingerprint = required_text(exact_fingerprint, "review fingerprint")?;
        if self.preview_fingerprint.as_deref() != Some(exact_fingerprint.as_str()) {
            return Err("review preview changed; create a new exact approval".to_string());
        }
        self.tool_invocation_id = Some(tool_invocation_id);
        self.status = ReviewQueueItemStatus::PendingApproval;
        self.revision = self.revision.saturating_add(1);
        self.updated_at = changed_at;
        Ok(())
    }

    pub fn resolve(&mut self, accepted: bool, changed_at: DateTime<Utc>) -> Result<(), String> {
        if !matches!(
            self.status,
            ReviewQueueItemStatus::PendingReview | ReviewQueueItemStatus::PendingApproval
        ) {
            return Err("review item is already resolved".to_string());
        }
        if accepted && self.status == ReviewQueueItemStatus::PendingApproval {
            return Err("external mutation must use its exact approval action".to_string());
        }
        self.status = if accepted {
            ReviewQueueItemStatus::Accepted
        } else {
            ReviewQueueItemStatus::Rejected
        };
        self.revision = self.revision.saturating_add(1);
        self.updated_at = changed_at;
        Ok(())
    }

    pub fn complete_approved_action(
        &mut self,
        evidence_ref: String,
        changed_at: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.status != ReviewQueueItemStatus::PendingApproval {
            return Err("review item is not waiting for an approved action".to_string());
        }
        self.evidence_ref = Some(required_text(evidence_ref, "review evidence")?);
        self.status = ReviewQueueItemStatus::Accepted;
        self.revision = self.revision.saturating_add(1);
        self.updated_at = changed_at;
        Ok(())
    }
}

pub fn automation_run_transition_allowed(
    from: AutomationRunStatus,
    to: AutomationRunStatus,
) -> bool {
    use AutomationRunStatus::*;
    matches!(
        (from, to),
        (Queued, Running)
            | (Queued, Cancelled)
            | (Queued, WaitingReview)
            | (Queued, WaitingApproval)
            | (Queued, Completed)
            | (Queued, Failed)
            | (Running, WaitingReview)
            | (Running, WaitingApproval)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, Cancelled)
            | (WaitingReview, WaitingApproval)
            | (WaitingReview, Completed)
            | (WaitingReview, Failed)
            | (WaitingReview, Cancelled)
            | (WaitingApproval, Completed)
            | (WaitingApproval, Failed)
            | (WaitingApproval, Cancelled)
            | (Failed, Queued)
    )
}

pub fn trigger_window_key(definition_id: Uuid, scheduled_for: DateTime<Utc>) -> String {
    format!("{definition_id}:{}", scheduled_for.to_rfc3339())
}

pub fn next_scheduled_at(
    definition: &AutomationDefinition,
    after: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    if let AutomationSchedule::Once { run_at } = definition.schedule {
        return Ok((run_at > after).then_some(run_at));
    }
    let timezone = definition
        .timezone
        .parse::<Tz>()
        .map_err(|_| "automation timezone is invalid".to_string())?;
    let local_after = after.with_timezone(&timezone);
    let start_date = local_after.date_naive();
    for offset in 0..=400 {
        let date = start_date + Duration::days(offset);
        let (hour, minute, matches_date) = match &definition.schedule {
            AutomationSchedule::Daily { hour, minute } => (*hour, *minute, true),
            AutomationSchedule::Weekly {
                weekday,
                hour,
                minute,
            } => (
                *hour,
                *minute,
                date.weekday().num_days_from_monday() == *weekday,
            ),
            AutomationSchedule::Monthly { day, hour, minute } => {
                (*hour, *minute, date.day() == *day)
            }
            AutomationSchedule::RestrictedCron {
                weekdays,
                hour,
                minute,
            } => (
                *hour,
                *minute,
                weekdays.is_empty() || weekdays.contains(&date.weekday().num_days_from_monday()),
            ),
            AutomationSchedule::Once { .. } => unreachable!(),
        };
        if !matches_date {
            continue;
        }
        let time = NaiveTime::from_hms_opt(hour, minute, 0)
            .ok_or_else(|| "automation schedule time is invalid".to_string())?;
        let local = date.and_time(time);
        let candidates = match timezone.from_local_datetime(&local) {
            LocalResult::Single(value) => vec![value.with_timezone(&Utc)],
            LocalResult::Ambiguous(first, second) => {
                let mut values = vec![first.with_timezone(&Utc), second.with_timezone(&Utc)];
                values.sort();
                values
            }
            LocalResult::None => Vec::new(),
        };
        if let Some(candidate) = candidates.into_iter().find(|value| *value > after) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn required_text(value: String, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_schedules_use_timezone_and_skip_nonexistent_dst_time() {
        let after = DateTime::parse_from_rfc3339("2026-03-06T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut definition = AutomationDefinition::once(
            "Daily task".to_string(),
            "America/New_York".to_string(),
            after,
        )
        .unwrap();
        definition.schedule = AutomationSchedule::Daily {
            hour: 2,
            minute: 30,
        };
        let next = next_scheduled_at(&definition, after).unwrap().unwrap();
        assert_eq!(
            next,
            DateTime::parse_from_rfc3339("2026-03-07T07:30:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        let after_first = DateTime::parse_from_rfc3339("2026-03-07T07:31:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let after_dst_gap = next_scheduled_at(&definition, after_first)
            .unwrap()
            .unwrap();
        assert_eq!(
            after_dst_gap,
            DateTime::parse_from_rfc3339("2026-03-09T06:30:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn weekly_monthly_and_restricted_cron_choose_next_matching_window() {
        let after = DateTime::parse_from_rfc3339("2026-07-12T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut definition =
            AutomationDefinition::once("Task".to_string(), "Asia/Shanghai".to_string(), after)
                .unwrap();
        definition.schedule = AutomationSchedule::Weekly {
            weekday: 0,
            hour: 9,
            minute: 0,
        };
        assert_eq!(
            next_scheduled_at(&definition, after).unwrap().unwrap(),
            DateTime::parse_from_rfc3339("2026-07-13T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        definition.schedule = AutomationSchedule::Monthly {
            day: 15,
            hour: 9,
            minute: 0,
        };
        assert_eq!(
            next_scheduled_at(&definition, after).unwrap().unwrap(),
            DateTime::parse_from_rfc3339("2026-07-15T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        definition.schedule = AutomationSchedule::RestrictedCron {
            weekdays: vec![1, 3],
            hour: 8,
            minute: 0,
        };
        assert_eq!(
            next_scheduled_at(&definition, after).unwrap().unwrap(),
            DateTime::parse_from_rfc3339("2026-07-14T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn resolved_review_items_cannot_be_reopened_by_edit_or_approval() {
        let now = Utc::now();
        let mut item = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id: Uuid::new_v4(),
            agent_run_id: None,
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some("sha256:frozen".to_string()),
            revision: 0,
            title: "Review".to_string(),
            evidence_ref: None,
            created_at: now,
            updated_at: now,
        };
        item.resolve(false, Utc::now()).expect("review rejects");
        assert!(item
            .edit(
                "Reopen".to_string(),
                Some("sha256:changed".to_string()),
                Utc::now()
            )
            .is_err());
        assert!(item
            .request_approval(Uuid::new_v4(), "sha256:frozen".to_string(), Utc::now())
            .is_err());
    }
}
