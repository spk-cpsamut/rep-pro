use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::dao::ResourceRow;

#[derive(Deserialize)]
pub(crate) struct CreateResourceRequest {
    pub username: String,
    pub password: SecretString,
    pub allow_upstream: bool,
    pub allow_downstream: bool,
    pub config: ResourceConfig,
}

#[derive(Deserialize)]
pub(crate) struct ResourceConfig {
    pub host: String,
    pub port: u16,
    pub database_name: String,
    pub table: String,
}

#[derive(Serialize)]
pub(crate) struct CreatedResource {
    pub resource_id: Uuid,
}

#[derive(Serialize)]
pub(crate) struct ResourceSummary {
    pub id: Uuid,
    pub allow_upstream: bool,
    pub allow_downstream: bool,
    pub created_at: String,
}

impl From<ResourceRow> for ResourceSummary {
    fn from(row: ResourceRow) -> Self {
        Self {
            id: row.id,
            allow_upstream: row.allow_upstream,
            allow_downstream: row.allow_downstream,
            created_at: row.created_at.to_rfc3339(),
        }
    }
}
