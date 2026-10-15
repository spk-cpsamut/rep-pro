CREATE TABLE persistent_keys (
    id UUID PRIMARY KEY NOT NULL DEFAULT uuidv7(),
    resource_id UUID NOT NULL REFERENCES resources(id),
    name VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (resource_id, name)
);

CREATE TABLE value_mappings (
    id UUID PRIMARY KEY NOT NULL DEFAULT uuidv7(),
    persistent_key_id UUID NOT NULL REFERENCES persistent_keys(id),
    source_identity TEXT NOT NULL,
    sanitize_type TEXT NOT NULL,
    target_value TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (persistent_key_id, source_identity, sanitize_type)
);

-- Rules created before persistent keys cannot reference one.
DELETE FROM rules;

ALTER TABLE rules
    ADD COLUMN persistent_key_id UUID NOT NULL REFERENCES persistent_keys(id);
