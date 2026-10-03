use std::collections::HashMap;

use axum::http::StatusCode;
use uuid::Uuid;

use crate::domain::http::{MAX_IDENT, internal_error, is_safe_ident};
use crate::domain::state::AppState;

use super::dao::Dao;
use super::models::{
    CreateResourceMapRequest, CreatedResourceMap, ResourceMapSummary, RuleSummary,
    pair_config_for_insert, stored_rule,
};

const NAMING_CONVENTIONS: &[&str] = &[
    "camel_case",
    "pascal_case",
    "snake_case",
    "screaming_case",
    "kebab_case",
    "train_case",
];

const SANITIZE_TYPES: &[&str] = &["name", "email", "phone_number", "ssn"];

pub(crate) async fn list_resource_maps(
    state: &AppState,
) -> Result<Vec<ResourceMapSummary>, (StatusCode, &'static str)> {
    let maps = state
        .resource_maps
        .list_maps()
        .await
        .map_err(|_| internal_error())?;
    let rules = state
        .resource_maps
        .list_rules()
        .await
        .map_err(|_| internal_error())?;

    let mut rules_by_map: HashMap<Uuid, Vec<RuleSummary>> = HashMap::new();
    for rule in rules {
        rules_by_map
            .entry(rule.resource_map_id)
            .or_default()
            .push(RuleSummary::from(rule));
    }

    Ok(maps
        .into_iter()
        .map(|row| {
            let rules = rules_by_map.remove(&row.id).unwrap_or_default();
            ResourceMapSummary::from_row(row, rules)
        })
        .collect())
}

pub(crate) async fn create_resource_map(
    state: &AppState,
    req: CreateResourceMapRequest,
) -> Result<CreatedResourceMap, (StatusCode, &'static str)> {
    validate_map_request(&req)?;
    let pair = pair_config_for_insert(&req.pair_config)?;

    let mut tx = state
        .resource_maps
        .begin()
        .await
        .map_err(|_| internal_error())?;
    let source = state
        .resource_maps
        .load_flags(&mut tx, req.resource_source)
        .await
        .map_err(|_| internal_error())?;
    let target = state
        .resource_maps
        .load_flags(&mut tx, req.resource_target)
        .await
        .map_err(|_| internal_error())?;
    let source = source.ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;
    let target = target.ok_or((StatusCode::BAD_REQUEST, "resource not found"))?;
    if !source.allow_downstream {
        return Err((StatusCode::BAD_REQUEST, "source does not allow downstream"));
    }
    if !target.allow_upstream {
        return Err((StatusCode::BAD_REQUEST, "target does not allow upstream"));
    }

    let resource_map_id = state
        .resource_maps
        .insert_map(
            &mut tx,
            req.resource_source,
            req.resource_target,
            &req.table_source,
            &req.table_target,
            &pair,
        )
        .await?;

    let mut rule_ids = Vec::with_capacity(req.rules.len());
    for rule in &req.rules {
        let rule_id = state
            .resource_maps
            .insert_rule(&mut tx, resource_map_id, stored_rule(rule))
            .await?;
        rule_ids.push(rule_id);
    }

    state
        .resource_maps
        .commit(tx)
        .await
        .map_err(|_| internal_error())?;
    Ok(CreatedResourceMap {
        resource_map_id,
        rule_ids,
    })
}

fn validate_map_request(req: &CreateResourceMapRequest) -> Result<(), (StatusCode, &'static str)> {
    if !valid_table(&req.table_source) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_source"));
    }
    if !valid_table(&req.table_target) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_target"));
    }
    if !is_safe_ident(&req.pair_config.pull_batch_field) {
        return Err((StatusCode::BAD_REQUEST, "invalid pull field"));
    }
    if req.rules.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "at least one rule is required"));
    }
    for rule in &req.rules {
        if !NAMING_CONVENTIONS.contains(&rule.source_naming_convention.as_str())
            || !NAMING_CONVENTIONS.contains(&rule.target_naming_convention.as_str())
        {
            return Err((StatusCode::BAD_REQUEST, "invalid naming convention"));
        }
        if !is_safe_ident(&rule.source_identifier) {
            return Err((StatusCode::BAD_REQUEST, "invalid source identifier"));
        }
        if rule.fields.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "each rule needs a field"));
        }
        for field in &rule.fields {
            if !is_safe_ident(&field.field_name) {
                return Err((StatusCode::BAD_REQUEST, "invalid field name"));
            }
            if !SANITIZE_TYPES.contains(&field.sanitize_type.as_str()) {
                return Err((StatusCode::BAD_REQUEST, "invalid sanitize type"));
            }
        }
    }
    Ok(())
}

fn valid_table(name: &str) -> bool {
    name.len() <= MAX_IDENT && is_safe_ident(name)
}

#[cfg(test)]
mod tests {
    use super::super::models::{FieldRequest, PairConfigRequest, RuleRequest};
    use super::*;
    use serde_json::Value;

    fn sample_request() -> CreateResourceMapRequest {
        CreateResourceMapRequest {
            resource_source: Uuid::nil(),
            resource_target: Uuid::nil(),
            table_source: "users".into(),
            table_target: "users_out".into(),
            pair_config: PairConfigRequest {
                pull_batch_strategy: "cursor".into(),
                pull_batch_field: "id".into(),
            },
            rules: vec![RuleRequest {
                force_pull_from_start: false,
                source_naming_convention: "snake_case".into(),
                target_naming_convention: "snake_case".into(),
                source_identifier: "id".into(),
                fields: vec![FieldRequest {
                    field_name: "email".into(),
                    sanitize_type: "email".into(),
                }],
            }],
        }
    }

    #[test]
    fn pair_config_starts_without_a_checkpoint() {
        let pair = pair_config_for_insert(&sample_request().pair_config).unwrap();
        let json = pair.to_json();
        assert_eq!(json["pull_batch_strategy"], "cursor");
        assert_eq!(json["pull_batch_field"], "id");
        assert_eq!(json["pull_batch_checkpoint"], Value::Null);
    }

    #[test]
    fn stored_rule_uses_domain_field_names() {
        let rule = stored_rule(&sample_request().rules[0]);
        assert_eq!(rule["source_identifier"], "id");
        assert_eq!(rule["source_naming_convention"], "snake_case");
        assert_eq!(rule["source_sanitize_fields"][0]["field_name"], "email");
        assert_eq!(rule["source_sanitize_fields"][0]["sanitize_type"], "email");
        assert!(rule.get("fields").is_none());
    }

    #[test]
    fn rejects_bad_link_input() {
        let mut req = sample_request();
        req.pair_config.pull_batch_strategy = "offset".into();
        assert!(pair_config_for_insert(&req.pair_config).is_err());

        req = sample_request();
        req.rules.clear();
        assert_eq!(
            validate_map_request(&req).unwrap_err().1,
            "at least one rule is required"
        );

        req = sample_request();
        req.table_source = "users-out".into();
        assert_eq!(
            validate_map_request(&req).unwrap_err().1,
            "invalid table_source"
        );
    }
}
