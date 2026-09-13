pub struct PostgresConfigJson {
    host: String,
    port: u16,
    username: String,
    password: String,
    database_name: String,
    table: String,
    pull_batch_strategy: String,
    pull_batch_field: String,
    pull_batch_checkpoint: Option<String>,
}

impl From<PostgresConfig> for PostgresConfigJson {
    fn from(value: PostgresConfig) -> Self {
        let (plan_strategy, field, checkpoint): (String, String, Option<String>) =
            match value.pull_batch_strategy {
                PullBatchStrategy::Cursor { field, pointer } => (CURSOR.to_owned(), field, pointer),
                PullBatchStrategy::LimitOffSet { field, offset } => {
                    let checkpoint = match offset {
                        Some(sql_offset) => Some(sql_offset.get().to_string()),
                        None => None,
                    };
                    (LIMIT_OFFSET.to_owned(), field, checkpoint)
                }
            };
        Self {
            host: value.host.as_ref().to_owned(),
            port: value.port,
            username: value.username.as_ref().expose_secret().to_string(),
            password: value.password.as_ref().expose_secret().to_string(),
            database_name: value.database_name.as_ref().to_owned(),
            table: value.table,
            pull_batch_strategy: plan_strategy,
            pull_batch_field: field,
            pull_batch_checkpoint: checkpoint,
        }
    }
}
