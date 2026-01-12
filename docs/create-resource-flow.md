# overall workflows

This documents provide comprehensive workflows in rep-pro, it act like the single source of truth as long as we not found something better

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
    participant C as Client
    end
    box internal
    participant S as Service
    end
    box Database
    participant R as resources
    participant RS as rules
    participant RSR as resources_relation
    end
    C ->> S: setMappingRules(source: resource_id, target: resource_id)
    S ->> RSR: insertResourcesRules(source, target)
    RSR -->> S: (resources_relation_id)
    S ->> RS: insertRules(rules, resources_relation_id)
```

<br>
<br>

### ER diagram

```mermaid
erDiagram
Resource {
    UUID id PK
    cipher_text username
    cipher_text password
    cipher_text config
    TIMESTAMP_WITH_TZ created_at
    TIMESTAMP_WITH_TZ updated_at
}
```

