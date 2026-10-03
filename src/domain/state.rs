use aws_sdk_kms::Client;
use sqlx::PgPool;

use super::resource::dao::ResourceDao;
use super::resource_map::dao::ResourceMapDao;

#[derive(Clone)]
pub struct AppState {
    pub resources: ResourceDao,
    pub resource_maps: ResourceMapDao,
    pub kms: Client,
    pub kms_key_id: String,
}

impl AppState {
    pub fn new(pool: PgPool, kms: Client, kms_key_id: String) -> Self {
        Self {
            resources: ResourceDao::new(pool.clone()),
            resource_maps: ResourceMapDao::new(pool),
            kms,
            kms_key_id,
        }
    }
}
