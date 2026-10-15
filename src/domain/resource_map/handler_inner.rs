use std::collections::HashMap;

use axum::http::StatusCode;
use serde_json::Value;
use uuid::Uuid;

use crate::domain::http::{MAX_IDENT, internal_error, is_safe_ident};
use crate::domain::orchestrator::{OrchestratorError, ParallelOrchestrator};
use crate::domain::resource::postgres_resource::fetch_sample_row;
use crate::domain::resource::{Record, Rules, connect_resource, load_postgres_resource};
use crate::domain::resource_map::{PairConfig, ResourceMap};
use crate::domain::sanitizer::persistent_sanitizer::{
    PersistentSanitizer, PostgresValueMappingStore,
};
use crate::domain::state::AppState;
use crate::infrastructure::persistence::PostgresResourceMapStore;

use super::dao::Dao;
use super::models::{
    CreateResourceMapRequest, CreatedResourceMap, ResourceMapSummary, RuleSummary, SanitizeExample,
    SanitizePreview, pair_config_for_insert, stored_rule,
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
        let persistent_key_id = state
            .resource_maps
            .find_persistent_key(&mut tx, req.resource_source, &rule.persistent_key)
            .await
            .map_err(|_| internal_error())?
            .ok_or((StatusCode::BAD_REQUEST, "persistent key not found"))?;
        let rule_id = state
            .resource_maps
            .insert_rule(
                &mut tx,
                resource_map_id,
                persistent_key_id,
                stored_rule(rule),
            )
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

pub(crate) async fn preview_sanitize(
    state: &AppState,
    id: Uuid,
) -> Result<SanitizePreview, (StatusCode, &'static str)> {
    let prepared = prepare_sanitize(state, id).await?;
    let sanitizer = persistent_sanitizer(state);
    examples_for(&prepared, &sanitizer).await
}

pub(crate) async fn run_sanitize(
    state: &AppState,
    id: Uuid,
) -> Result<SanitizePreview, (StatusCode, &'static str)> {
    let prepared = prepare_sanitize(state, id).await?;
    if !valid_table(&prepared.table_target) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_target"));
    }
    let sanitizer = persistent_sanitizer(state);
    let source =
        load_postgres_resource(state, prepared.resource_source, &prepared.table_source).await?;
    let target =
        load_postgres_resource(state, prepared.resource_target, &prepared.table_target).await?;
    let store = PostgresResourceMapStore::new(state.resource_maps.pool().clone());
    for rules in &prepared.rules {
        let resource_map = ResourceMap::new(
            prepared.resource_map_id,
            prepared.resource_source,
            prepared.resource_target,
            prepared.table_source.clone(),
            prepared.table_target.clone(),
            prepared.pair.clone(),
        );
        let mut orchestrator =
            ParallelOrchestrator::new(source.clone(), target.clone(), resource_map, store.clone());
        orchestrator
            .run(rules.clone(), sanitizer.clone())
            .await
            .map_err(orchestrator_error)?;
    }
    examples_for(&prepared, &sanitizer).await
}

struct PreparedSanitize {
    resource_map_id: Uuid,
    resource_source: Uuid,
    resource_target: Uuid,
    table_source: String,
    table_target: String,
    pair: PairConfig,
    rules: Vec<Rules>,
    columns: Vec<String>,
    row: Value,
}

async fn prepare_sanitize(
    state: &AppState,
    id: Uuid,
) -> Result<PreparedSanitize, (StatusCode, &'static str)> {
    let maps = list_resource_maps(state).await?;
    let map = maps
        .into_iter()
        .find(|map| map.resource_map_id == id)
        .ok_or((StatusCode::BAD_REQUEST, "resource map not found"))?;
    if map.rules.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "resource map has no rules"));
    }
    if !valid_table(&map.table_source) {
        return Err((StatusCode::BAD_REQUEST, "invalid table_source"));
    }
    let pull_field = map
        .pair_config
        .get("pull_batch_field")
        .and_then(Value::as_str)
        .ok_or(internal_error())?
        .to_owned();
    if !is_safe_ident(&pull_field) {
        return Err((StatusCode::BAD_REQUEST, "invalid pull field"));
    }
    let pair = PairConfig::from_json(map.pair_config).map_err(|_| internal_error())?;

    let mut rules = Vec::with_capacity(map.rules.len());
    for entry in &map.rules {
        let parsed = Rules::from_stored(entry.rule_id, entry.persistent_key_id, &entry.rule)
            .map_err(|_| internal_error())?;
        require_ident(
            &parsed
                .get_source_identifier()
                .get_field_identifer_as_string(),
            "invalid source identifier",
        )?;
        for field in parsed.get_source_field() {
            require_ident(field.get_field_name(), "invalid field name")?;
        }
        rules.push(parsed);
    }

    let pool = connect_resource(state, map.resource_source).await?;
    let fetched = fetch_sample_row(&pool, &map.table_source, &pull_field).await;
    pool.close().await;
    let fetched = fetched.map_err(|_| (StatusCode::BAD_REQUEST, "cannot query table"))?;
    let (mut columns, row) = fetched.ok_or((StatusCode::BAD_REQUEST, "source table is empty"))?;
    for rule in &rules {
        for field in rule.get_source_field() {
            let name = field.get_field_name();
            if !columns.iter().any(|column| column == name) {
                columns.push(name.to_owned());
            }
        }
    }

    Ok(PreparedSanitize {
        resource_map_id: map.resource_map_id,
        resource_source: map.resource_source,
        resource_target: map.resource_target,
        table_source: map.table_source,
        table_target: map.table_target,
        pair,
        rules,
        columns,
        row,
    })
}

fn persistent_sanitizer(state: &AppState) -> PersistentSanitizer {
    PersistentSanitizer::new(PostgresValueMappingStore::new(
        state.resource_maps.pool().clone(),
    ))
}

async fn examples_for(
    prepared: &PreparedSanitize,
    sanitizer: &PersistentSanitizer,
) -> Result<SanitizePreview, (StatusCode, &'static str)> {
    let mut examples = Vec::with_capacity(prepared.rules.len());
    for rules in &prepared.rules {
        examples.push(preview_rule(rules.clone(), &prepared.row, sanitizer).await?);
    }
    Ok(SanitizePreview {
        columns: prepared.columns.clone(),
        examples,
    })
}

async fn preview_rule(
    rules: Rules,
    row: &Value,
    sanitizer: &PersistentSanitizer,
) -> Result<SanitizeExample, (StatusCode, &'static str)> {
    let rule_id = *rules.get_rule_id();
    let field_names: Vec<String> = rules
        .get_source_field()
        .iter()
        .map(|field| field.get_field_name().to_owned())
        .collect();
    let sanitized = sanitizer
        .preview(vec![Record::from_value(row.clone())], rules)
        .await
        .map_err(|_| internal_error())?;
    let after = sanitized.into_iter().next().ok_or(internal_error())?;
    Ok(SanitizeExample {
        rule_id,
        before: row.clone(),
        after: overlay_sanitized(row, &after, &field_names),
    })
}

fn overlay_sanitized(row: &Value, sanitized: &Value, fields: &[String]) -> Value {
    let mut after = row.as_object().cloned().unwrap_or_default();
    for name in fields {
        after.insert(
            name.clone(),
            sanitized.get(name).cloned().unwrap_or(Value::Null),
        );
    }
    Value::Object(after)
}

fn orchestrator_error(err: OrchestratorError) -> (StatusCode, &'static str) {
    match err {
        OrchestratorError::DownstreamNotAllowed => {
            (StatusCode::BAD_REQUEST, "source does not allow downstream")
        }
        OrchestratorError::UpstreamNotAllowed => {
            (StatusCode::BAD_REQUEST, "target does not allow upstream")
        }
        OrchestratorError::SourceConnectionFailed | OrchestratorError::TargetConnectionFailed => {
            (StatusCode::BAD_REQUEST, "cannot connect to database")
        }
        OrchestratorError::PullFailed => (StatusCode::BAD_REQUEST, "cannot query table"),
        OrchestratorError::SanitizeFailed => (StatusCode::BAD_REQUEST, "sanitize failed"),
        OrchestratorError::PushFailed => (StatusCode::BAD_REQUEST, "cannot write to target"),
        OrchestratorError::SyncUpConfigFailed => internal_error(),
    }
}

fn require_ident(name: &str, message: &'static str) -> Result<(), (StatusCode, &'static str)> {
    if name.len() > MAX_IDENT || !is_safe_ident(name) {
        return Err((StatusCode::BAD_REQUEST, message));
    }
    Ok(())
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
        if rule.persistent_key.len() > MAX_IDENT || !is_safe_ident(&rule.persistent_key) {
            return Err((StatusCode::BAD_REQUEST, "invalid persistent key"));
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
                persistent_key: "customer".into(),
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
        assert_eq!(rule["persistent_key"], "customer");
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

        req = sample_request();
        req.rules[0].persistent_key = "not a key".into();
        assert_eq!(
            validate_map_request(&req).unwrap_err().1,
            "invalid persistent key"
        );
    }

    #[tokio::test]
    async fn preview_keeps_identifier_and_replaces_sanitized_fields() {
        let rule_id = Uuid::from_u128(1);
        let rule = Rules::from_stored(
            rule_id,
            Uuid::from_u128(2),
            &serde_json::json!({
                "force_pull_from_start": false,
                "source_naming_convention": "snake_case",
                "target_naming_convention": "camel_case",
                "source_identifier": "id",
                "source_sanitize_fields": [{
                    "field_name": "email",
                    "sanitize_type": "email"
                }]
            }),
        )
        .unwrap();
        let sanitizer = PersistentSanitizer::new(
            crate::domain::sanitizer::persistent_sanitizer::MemoryValueMappingStore::default(),
        );
        let row = serde_json::json!({
            "id": "42",
            "email": "jane@acme.com",
            "extra": "unchanged"
        });

        let example = preview_rule(rule, &row, &sanitizer).await.unwrap();

        assert_eq!(example.rule_id, rule_id);
        assert_eq!(example.before["id"], "42");
        assert_eq!(example.before["email"], "jane@acme.com");
        assert_eq!(example.before["extra"], "unchanged");
        assert_eq!(example.after["id"], "42");
        assert_eq!(example.after["extra"], "unchanged");
        assert_ne!(example.after["email"], "jane@acme.com");
        assert!(
            example.after["email"]
                .as_str()
                .is_some_and(|email| email.contains('@'))
        );
    }
}
