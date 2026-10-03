ALTER TABLE resources
    ALTER COLUMN username TYPE BYTEA USING convert_to(username, 'UTF8'),
    ALTER COLUMN password TYPE BYTEA USING convert_to(password, 'UTF8'),
    ALTER COLUMN config TYPE BYTEA USING convert_to(config::text, 'UTF8');

ALTER TABLE resources
    ADD COLUMN allow_upstream BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN allow_downstream BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE resources
    ALTER COLUMN allow_upstream DROP DEFAULT,
    ALTER COLUMN allow_downstream DROP DEFAULT;
