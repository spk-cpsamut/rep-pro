ALTER TABLE resources DROP COLUMN allow_upstream;
ALTER TABLE resources DROP COLUMN allow_downstream;

ALTER TABLE resources
    ALTER COLUMN username TYPE VARCHAR USING convert_from(username, 'UTF8'),
    ALTER COLUMN password TYPE VARCHAR USING convert_from(password, 'UTF8'),
    ALTER COLUMN config TYPE JSONB USING convert_from(config, 'UTF8')::jsonb;
