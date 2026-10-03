use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::PairConfig;
use super::dao::{ResourceMapRow, RuleRow};

#[derive(Deserialize)]
pub(crate) struct CreateResourceMapRequest {
    pub resource_source: Uuid,
    pub resource_target: Uuid,
    pub table_source: String,
    pub table_target: String,
    pub pair_config: PairConfigRequest,
    pub rules: Vec<RuleRequest>,
}

#[derive(Deserialize)]
pub(crate) struct PairConfigRequest {
    pub pull_batch_strategy: String,
    pub pull_batch_field: String,
}

#[derive(Deserialize)]
pub(crate) struct RuleRequest {
    pub force_pull_from_start: bool,
    pub source_naming_convention: String,
    pub target_naming_convention: String,
    pub source_identifier: String,
    pub fields: Vec<FieldRequest>,
}

#[derive(Deserialize)]
pub(crate) struct FieldRequest {
    pub field_name: String,
    pub sanitize_type: String,
}

#[derive(Serialize)]
pub(crate) struct CreatedResourceMap {
    pub resource_map_id: Uuid,
    pub rule_ids: Vec<Uuid>,
}

#[derive(Serialize)]
pub(crate) struct ResourceMapSummary {
    pub resource_map_id: Uuid,
    pub resource_source: Uuid,
    pub resource_target: Uuid,
    pub table_source: String,
    pub table_target: String,
    pub pair_config: Value,
    pub rules: Vec<RuleSummary>,
}

#[derive(Serialize)]
pub(crate) struct RuleSummary {
    pub rule_id: Uuid,
    pub rule: Value,
}

impl ResourceMapSummary {
    pub(crate) fn from_row(row: ResourceMapRow, rules: Vec<RuleSummary>) -> Self {
        Self {
            resource_map_id: row.id,
            resource_source: row.resource_source,
            resource_target: row.resource_target,
            table_source: row.table_source,
            table_target: row.table_target,
            pair_config: row.pair_config.0,
            rules,
        }
    }
}

impl From<RuleRow> for RuleSummary {
    fn from(row: RuleRow) -> Self {
        Self {
            rule_id: row.id,
            rule: row.rule.0,
        }
    }
}

pub(crate) fn pair_config_for_insert(
    req: &PairConfigRequest,
) -> Result<PairConfig, (StatusCode, &'static str)> {
    let value = json!({
        "pull_batch_strategy": req.pull_batch_strategy,
        "pull_batch_field": req.pull_batch_field,
        "pull_batch_checkpoint": null,
    });
    PairConfig::from_json(value).map_err(|_| (StatusCode::BAD_REQUEST, "invalid pull strategy"))
}

pub(crate) fn stored_rule(rule: &RuleRequest) -> Value {
    json!({
        "force_pull_from_start": rule.force_pull_from_start,
        "source_naming_convention": rule.source_naming_convention,
        "target_naming_convention": rule.target_naming_convention,
        "source_identifier": rule.source_identifier,
        "source_sanitize_fields": rule.fields.iter().map(|field| json!({
            "field_name": field.field_name,
            "sanitize_type": field.sanitize_type,
        })).collect::<Vec<_>>(),
    })
}
