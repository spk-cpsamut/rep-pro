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

#[cfg(test)]
pub(crate) fn idle_state() -> AppState {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://localhost/unused")
        .expect("lazy pool");
    let config = aws_sdk_kms::Config::builder()
        .behavior_version(aws_sdk_kms::config::BehaviorVersion::latest())
        .region(aws_sdk_kms::config::Region::new("us-east-1"))
        .credentials_provider(aws_sdk_kms::config::Credentials::for_tests())
        .build();
    AppState::new(pool, Client::from_conf(config), "key".into())
}
