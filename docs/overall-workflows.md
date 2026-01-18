# overall workflows

This documents provide comprehensive workflows in rep-pro, it act like the single source of truth.

## Create resource

```mermaid
sequenceDiagram
    box Client
    participant C as Client
    end
    box internal
    participant S as Service
    participant R as resources DB
    end
    box external
    participant K as AWS KMS
    end
    C ->> S: 1. create resource (username, password, config)
    S ->> K: 2. Encrypt (username, password, config)
    K -->> S: 3. (encrypted data)
    S ->> R: 4. insert resource with encrypted data
    R -->> S: 5. (resource_id)
    S -->> C: 6. (resource_id)
```

### On step 1 we can insert one more step to either validate username, password, config to check whether this user has enough permissions or required admin roles to use this user generate enough role permissions

<br>
<br>
<br>
<br>

## Set Mapping rules

```mermaid
sequenceDiagram
    box
    actor C as Client
    end
    box internal
    participant S as Service
    end
    box Database
    participant R as resources
    participant RS as rules
    participant RSM as resources_maps
    end
    C ->> S: setMappingRules(source: {resource_id, table}, target: {resource_id, table}, rules[])
    S ->> RSM : insert resource_map(source: {resource_id, table}, target: {resource_id, table})
    RSM -->> S : resource_map_id
    S ->> RS: insertRules(resource_map_id, rules[])
```

<br>
<br>

## ER diagram

```mermaid
erDiagram
resource {
    UUID id PK
    cipher_text username
    cipher_text password
    cipher_text config
    TIMESTAMP_WITH_TZ created_at
    TIMESTAMP_WITH_TZ updated_at
}

resource_map {
    UUID id PK
    UUID resource_source FK
    UUID resource_target FK
    VARCHAR225 table_source
    VARCHAR255 table_target
}

rule {
    UUID id PK
    UUID resource_map_id
    JSONB rule
}

resource }o -- o{ resource_map: source
resource }o -- o{ resource_map: target

resource_map || -- |{ rule: has
```
