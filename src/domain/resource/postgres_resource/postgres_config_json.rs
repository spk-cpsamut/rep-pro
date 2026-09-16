use secrecy::ExposeSecret;

use crate::domain::resource::postgres_resource::postgres_config::PostgresConfig;

pub struct PostgresConfigJson {
    host: String,
    port: u16,
    username: String,
    password: String,
    database_name: String,
    table: String,
}

impl From<PostgresConfig> for PostgresConfigJson {
    fn from(value: PostgresConfig) -> Self {
        Self {
            host: value.host.as_ref().to_owned(),
            port: value.port,
            username: value.username.as_ref().expose_secret().to_string(),
            password: value.password.as_ref().expose_secret().to_string(),
            database_name: value.database_name.as_ref().to_owned(),
            table: value.table,
        }
    }
}
