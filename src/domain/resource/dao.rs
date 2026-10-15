use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait::async_trait]
pub(crate) trait Dao: Send + Sync {
    async fn list(&self) -> Result<Vec<ResourceRow>, sqlx::Error>;

    async fn insert(
        &self,
        username: &[u8],
        password: &[u8],
        config: &[u8],
        allow_upstream: bool,
        allow_downstream: bool,
    ) -> Result<Uuid, sqlx::Error>;

    async fn load_ciphertext(&self, id: Uuid) -> Result<Option<ResourceCiphertext>, sqlx::Error>;
}

#[derive(Clone)]
pub struct ResourceDao {
    pool: PgPool,
}

impl ResourceDao {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) async fn resource_exists(&self, id: Uuid) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM resources WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await
    }

    pub(crate) async fn list_persistent_keys(
        &self,
        resource_id: Uuid,
    ) -> Result<Vec<PersistentKeyRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, name FROM persistent_keys WHERE resource_id = $1 ORDER BY name ASC",
        )
        .bind(resource_id)
        .fetch_all(&self.pool)
        .await
    }

    pub(crate) async fn insert_persistent_key(
        &self,
        resource_id: Uuid,
        name: &str,
    ) -> Result<PersistentKeyRow, sqlx::Error> {
        sqlx::query_as(
            "INSERT INTO persistent_keys (resource_id, name) VALUES ($1, $2) RETURNING id, name",
        )
        .bind(resource_id)
        .bind(name)
        .fetch_one(&self.pool)
        .await
    }
}

#[async_trait::async_trait]
impl Dao for ResourceDao {
    async fn list(&self) -> Result<Vec<ResourceRow>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, allow_upstream, allow_downstream, created_at FROM resources ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn insert(
        &self,
        username: &[u8],
        password: &[u8],
        config: &[u8],
        allow_upstream: bool,
        allow_downstream: bool,
    ) -> Result<Uuid, sqlx::Error> {
        sqlx::query_scalar(
            "INSERT INTO resources (username, password, config, allow_upstream, allow_downstream) VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(username)
        .bind(password)
        .bind(config)
        .bind(allow_upstream)
        .bind(allow_downstream)
        .fetch_one(&self.pool)
        .await
    }

    async fn load_ciphertext(&self, id: Uuid) -> Result<Option<ResourceCiphertext>, sqlx::Error> {
        sqlx::query_as("SELECT username, password, config FROM resources WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }
}

#[derive(sqlx::FromRow)]
pub(crate) struct ResourceCiphertext {
    pub username: Vec<u8>,
    pub password: Vec<u8>,
    pub config: Vec<u8>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct PersistentKeyRow {
    pub id: Uuid,
    pub name: String,
}

#[derive(sqlx::FromRow)]
pub(crate) struct ResourceRow {
    pub id: Uuid,
    pub allow_upstream: bool,
    pub allow_downstream: bool,
    pub created_at: DateTime<Utc>,
}
