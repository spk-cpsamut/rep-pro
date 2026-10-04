const LABEL_KEY = "rep-pro.resource-labels";

export const NAMING_CONVENTIONS = [
  "camel_case",
  "pascal_case",
  "snake_case",
  "screaming_case",
  "kebab_case",
  "train_case",
] as const;

export const SANITIZE_TYPES = ["name", "email", "phone_number", "ssn"] as const;

export const PULL_STRATEGIES = ["cursor", "LimitOffSet"] as const;

export type NamingConvention = (typeof NAMING_CONVENTIONS)[number];
export type SanitizeType = (typeof SANITIZE_TYPES)[number];
export type PullStrategy = (typeof PULL_STRATEGIES)[number];

export type ResourceSummary = {
  id: string;
  allow_upstream: boolean;
  allow_downstream: boolean;
  created_at: string;
};

export type ResourceLabel = {
  label: string;
  table: string;
};

export type FieldInput = {
  field_name: string;
  sanitize_type: SanitizeType;
};

export type RuleInput = {
  force_pull_from_start: boolean;
  source_naming_convention: NamingConvention;
  target_naming_convention: NamingConvention;
  source_identifier: string;
  fields: FieldInput[];
};

export type CreateResourceInput = {
  username: string;
  password: string;
  allow_upstream: boolean;
  allow_downstream: boolean;
  host: string;
  port: number;
  database_name: string;
  table: string;
};

export type CreateResourceMapInput = {
  resource_source: string;
  resource_target: string;
  table_source: string;
  table_target: string;
  pull_batch_strategy: PullStrategy;
  pull_batch_field: string;
  rules: RuleInput[];
};

export type StoredField = {
  field_name: string;
  sanitize_type: string;
};

export type StoredRule = {
  force_pull_from_start: boolean;
  source_naming_convention: string;
  target_naming_convention: string;
  source_identifier: string;
  source_sanitize_fields: StoredField[];
};

export type ColumnMeta = {
  column_name: string;
  data_type: string;
  is_primary_key: boolean;
};

export type SanitizeExample = {
  rule_id: string;
  before: Record<string, unknown>;
  after: Record<string, unknown>;
};

export type SanitizePreview = {
  columns: string[];
  examples: SanitizeExample[];
};

export type ResourceMapSummary = {
  resource_map_id: string;
  resource_source: string;
  resource_target: string;
  table_source: string;
  table_target: string;
  pair_config: {
    pull_batch_strategy: string;
    pull_batch_field: string;
    pull_batch_checkpoint: string | null;
  };
  rules: { rule_id: string; rule: StoredRule }[];
};

export function shortId(id: string): string {
  return id.slice(0, 8);
}

export function resourceTitle(
  id: string,
  labels: Record<string, ResourceLabel>,
): string {
  return labels[id]?.label ?? shortId(id);
}

export function readLabels(): Record<string, ResourceLabel> {
  try {
    const raw = localStorage.getItem(LABEL_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return {};
    const labels: Record<string, ResourceLabel> = {};
    for (const [id, value] of Object.entries(parsed)) {
      if (!value || typeof value !== "object") continue;
      const record = value as Record<string, unknown>;
      if (typeof record.label === "string" && typeof record.table === "string") {
        labels[id] = { label: record.label, table: record.table };
      }
    }
    return labels;
  } catch {
    return {};
  }
}

export function rememberLabel(id: string, label: ResourceLabel) {
  const labels = readLabels();
  labels[id] = label;
  localStorage.setItem(LABEL_KEY, JSON.stringify(labels));
}

export function emptyRule(): RuleInput {
  return {
    force_pull_from_start: false,
    source_naming_convention: "snake_case",
    target_naming_convention: "snake_case",
    source_identifier: "id",
    fields: [{ field_name: "", sanitize_type: "name" }],
  };
}

export async function listColumns(
  resourceId: string,
  table: string,
  schema = "public",
): Promise<ColumnMeta[]> {
  const params = new URLSearchParams({ schema, table });
  return request<ColumnMeta[]>(`/resources/${resourceId}/columns?${params.toString()}`);
}

export async function listResources(): Promise<ResourceSummary[]> {
  return request<ResourceSummary[]>("/resources");
}

export async function listResourceMaps(): Promise<ResourceMapSummary[]> {
  return request<ResourceMapSummary[]>("/resource-maps");
}

export async function createResource(
  input: CreateResourceInput,
): Promise<{ resource_id: string }> {
  return request("/resources", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      username: input.username,
      password: input.password,
      allow_upstream: input.allow_upstream,
      allow_downstream: input.allow_downstream,
      config: {
        host: input.host,
        port: input.port,
        database_name: input.database_name,
        table: input.table,
      },
    }),
  });
}

export async function previewSanitize(id: string): Promise<SanitizePreview> {
  return request(`/resource-maps/${id}/sanitize-preview`, { method: "POST" });
}

export async function createResourceMap(
  input: CreateResourceMapInput,
): Promise<{ resource_map_id: string; rule_ids: string[] }> {
  return request("/resource-maps", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      resource_source: input.resource_source,
      resource_target: input.resource_target,
      table_source: input.table_source,
      table_target: input.table_target,
      pair_config: {
        pull_batch_strategy: input.pull_batch_strategy,
        pull_batch_field: input.pull_batch_field,
      },
      rules: input.rules,
    }),
  });
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, init);
  if (!response.ok) {
    const text = (await response.text()).trim();
    throw new Error(text || response.statusText);
  }
  return (await response.json()) as T;
}
