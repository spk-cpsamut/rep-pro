CREATE TABLE resource_maps (
    id UUID PRIMARY KEY NOT NULL DEFAULT uuidv7(),
    resource_source UUID NOT NULL REFERENCES resources(id),
    resource_target UUID NOT NULL REFERENCES resources(id),
    table_source VARCHAR(255) NOT NULL,
    table_target VARCHAR(255) NOT NULL,
    pair_config JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (resource_source, resource_target, table_source, table_target)
);
