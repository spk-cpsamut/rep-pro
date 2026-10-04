import { useEffect, useState, type FormEvent } from "react";
import {
  NAMING_CONVENTIONS,
  PULL_STRATEGIES,
  SANITIZE_TYPES,
  createResource,
  createResourceMap,
  emptyRule,
  listColumns,
  rememberLabel,
  resourceTitle,
  type ColumnMeta,
  type FieldInput,
  type NamingConvention,
  type PullStrategy,
  type ResourceLabel,
  type ResourceMapSummary,
  type ResourceSummary,
  type RuleInput,
  type SanitizeType,
} from "./api.ts";

type PanelProps = {
  resources: ResourceSummary[];
  maps: ResourceMapSummary[];
  labels: Record<string, ResourceLabel>;
  selectedResourceId: string | null;
  selectedMapId: string | null;
  error: string | null;
  onSelectMap: (id: string) => void;
  onCreated: (id: string, label: ResourceLabel) => void;
  onLinked: () => void;
  onError: (message: string | null) => void;
};

export function Panel({
  resources,
  maps,
  labels,
  selectedResourceId,
  selectedMapId,
  error,
  onSelectMap,
  onCreated,
  onLinked,
  onError,
}: PanelProps) {
  const [tab, setTab] = useState<"resource" | "link">("resource");
  const sources = resources.filter((resource) => resource.allow_downstream);
  const targets = resources.filter((resource) => resource.allow_upstream);
  const selected = resources.find((resource) => resource.id === selectedResourceId);
  const selectedMap = maps.find((map) => map.resource_map_id === selectedMapId);

  useEffect(() => {
    if (selectedMapId) setTab("link");
  }, [selectedMapId]);

  return (
    <aside className="panel">
      <header className="panel-header">
        <p className="eyebrow">rep-pro</p>
        <h1>Resources</h1>
        <p className="lede">
          {tab === "resource"
            ? "Register a database so it can appear in the scene."
            : "Draw a link from a source table to a target table."}
        </p>
      </header>

      <ul className="legend">
        <li><i className="swatch copper" /> Source</li>
        <li><i className="swatch tide" /> Target</li>
        <li><i className="swatch sand" /> Both</li>
      </ul>

      <div className="tabs" role="tablist" aria-label="Resource panel">
        <button
          type="button"
          role="tab"
          id="tab-resource"
          aria-selected={tab === "resource"}
          aria-controls="panel-resource"
          className={tab === "resource" ? "active" : undefined}
          onClick={() => setTab("resource")}
        >
          Resource
        </button>
        <button
          type="button"
          role="tab"
          id="tab-link"
          aria-selected={tab === "link"}
          aria-controls="panel-link"
          className={tab === "link" ? "active" : undefined}
          onClick={() => setTab("link")}
        >
          Link
        </button>
      </div>

      {error ? <p className="banner error">{error}</p> : null}

      <div
        role="tabpanel"
        id="panel-resource"
        aria-labelledby="tab-resource"
        hidden={tab !== "resource"}
      >
        <CreateForm
          onCreated={onCreated}
          onError={onError}
        />
      </div>
      <div
        role="tabpanel"
        id="panel-link"
        aria-labelledby="tab-link"
        hidden={tab !== "link"}
      >
        <LinkForm
          sources={sources}
          targets={targets}
          labels={labels}
          selected={selected}
          onLinked={onLinked}
          onError={onError}
        />

        <section className="block">
          <h2>Links</h2>
          {maps.length === 0 ? (
            <p className="hint">No links yet. A new link shows up as a curve between two resources.</p>
          ) : (
            <ul className="link-list">
              {maps.map((map) => (
                <li key={map.resource_map_id}>
                  <button
                    type="button"
                    className={map.resource_map_id === selectedMapId ? "link-row selected" : "link-row"}
                    onClick={() => onSelectMap(map.resource_map_id)}
                  >
                    <span>
                      {resourceTitle(map.resource_source, labels)}.{map.table_source}
                    </span>
                    <span className="arrow">to</span>
                    <span>
                      {resourceTitle(map.resource_target, labels)}.{map.table_target}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {selectedMap ? <LinkDetail map={selectedMap} /> : null}
        </section>
      </div>
    </aside>
  );
}

function CreateForm({
  onCreated,
  onError,
}: {
  onCreated: (id: string, label: ResourceLabel) => void;
  onError: (message: string | null) => void;
}) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("5432");
  const [databaseName, setDatabaseName] = useState("");
  const [table, setTable] = useState("");
  const [allowUpstream, setAllowUpstream] = useState(false);
  const [allowDownstream, setAllowDownstream] = useState(false);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    const parsedPort = Number(port);
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      onError("Port must be between 1 and 65535");
      return;
    }
    setPending(true);
    onError(null);
    setNotice(null);
    try {
      const created = await createResource({
        username,
        password,
        allow_upstream: allowUpstream,
        allow_downstream: allowDownstream,
        host,
        port: parsedPort,
        database_name: databaseName,
        table,
      });
      const label = {
        label: `${host} / ${databaseName} / ${table}`,
        table,
      };
      rememberLabel(created.resource_id, label);
      onCreated(created.resource_id, label);
      setPassword("");
      setNotice(`Created ${created.resource_id}`);
    } catch (err) {
      onError(err instanceof Error ? err.message : "Could not create resource");
    } finally {
      setPending(false);
    }
  }

  return (
    <form className="block" onSubmit={(event) => void onSubmit(event)}>
      <h2>Create resource</h2>
      <label>
        Username
        <input value={username} onChange={(event) => setUsername(event.target.value)} required />
      </label>
      <label>
        Password
        <input
          type="password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          required
        />
      </label>
      <div className="split">
        <label>
          Host
          <input value={host} onChange={(event) => setHost(event.target.value)} required />
        </label>
        <label>
          Port
          <input
            inputMode="numeric"
            value={port}
            onChange={(event) => setPort(event.target.value)}
            required
          />
        </label>
      </div>
      <div className="split">
        <label>
          Database
          <input
            value={databaseName}
            onChange={(event) => setDatabaseName(event.target.value)}
            required
          />
        </label>
        <label>
          Table
          <input value={table} onChange={(event) => setTable(event.target.value)} required />
        </label>
      </div>
      <label className="check">
        <input
          type="checkbox"
          checked={allowDownstream}
          onChange={(event) => setAllowDownstream(event.target.checked)}
        />
        Allow downstream (can be a link source)
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={allowUpstream}
          onChange={(event) => setAllowUpstream(event.target.checked)}
        />
        Allow upstream (can be a link target)
      </label>
      <button type="submit" disabled={pending}>
        {pending ? "Creating…" : "Create resource"}
      </button>
      {notice ? <p className="banner ok">{notice}</p> : null}
    </form>
  );
}

function LinkForm({
  sources,
  targets,
  labels,
  selected,
  onLinked,
  onError,
}: {
  sources: ResourceSummary[];
  targets: ResourceSummary[];
  labels: Record<string, ResourceLabel>;
  selected: ResourceSummary | undefined;
  onLinked: () => void;
  onError: (message: string | null) => void;
}) {
  const [sourceId, setSourceId] = useState("");
  const [targetId, setTargetId] = useState("");
  const [tableSource, setTableSource] = useState("");
  const [tableTarget, setTableTarget] = useState("");
  const [strategy, setStrategy] = useState<PullStrategy>("cursor");
  const [pullField, setPullField] = useState("id");
  const [rules, setRules] = useState<RuleInput[]>([emptyRule()]);
  const [columns, setColumns] = useState<ColumnMeta[]>([]);
  const [columnHint, setColumnHint] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const columnMessage =
    columnHint ??
    (sourceId && tableSource && !isSafeTable(tableSource)
      ? "Table name must be a plain identifier."
      : null);

  useEffect(() => {
    if (!sourceId || !isSafeTable(tableSource)) return;

    let cancelled = false;
    const handle = window.setTimeout(() => {
      setColumnHint("Loading columns…");
      void listColumns(sourceId, tableSource)
        .then((next) => {
          if (cancelled) return;
          const names = new Set(next.map((column) => column.column_name));
          setColumns(next);
          setColumnHint(next.length === 0 ? "No columns found for this table." : null);
          setPullField((current) => (names.has(current) ? current : ""));
          setRules((current) =>
            current.map((rule) => ({
              ...rule,
              source_identifier: names.has(rule.source_identifier) ? rule.source_identifier : "",
              fields: rule.fields.map((field) => ({
                ...field,
                field_name: names.has(field.field_name) ? field.field_name : "",
              })),
            })),
          );
        })
        .catch((err: unknown) => {
          if (cancelled) return;
          setColumns([]);
          setColumnHint(err instanceof Error ? err.message : "Could not load columns");
        });
    }, 300);

    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [sourceId, tableSource]);

  function assignSelection(role: "source" | "target") {
    if (!selected) return;
    const knownTable = labels[selected.id]?.table ?? "";
    if (role === "source") {
      if (!selected.allow_downstream) return;
      const nextTable = tableSource || knownTable;
      if (sourceId !== selected.id || tableSource !== nextTable) {
        setColumns([]);
        setColumnHint(null);
      }
      setSourceId(selected.id);
      setTableSource(nextTable);
      return;
    }
    if (!selected.allow_upstream) return;
    setTargetId(selected.id);
    setTableTarget((current) => current || knownTable);
  }

  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setPending(true);
    onError(null);
    setNotice(null);
    try {
      const created = await createResourceMap({
        resource_source: sourceId,
        resource_target: targetId,
        table_source: tableSource,
        table_target: tableTarget,
        pull_batch_strategy: strategy,
        pull_batch_field: pullField,
        rules,
      });
      onLinked();
      setNotice(`Linked ${created.resource_map_id}`);
    } catch (err) {
      onError(err instanceof Error ? err.message : "Could not link resources");
    } finally {
      setPending(false);
    }
  }

  return (
    <form className="block" onSubmit={(event) => void onSubmit(event)}>
      <h2>Link resources</h2>
      <p className="hint">
        {selected
          ? `Selected ${resourceTitle(selected.id, labels)}. Use it as a source or a target.`
          : "Click a resource in the scene, then assign it."}
      </p>
      <div className="split">
        <button
          type="button"
          className="ghost"
          disabled={!selected?.allow_downstream}
          onClick={() => assignSelection("source")}
        >
          Use as source
        </button>
        <button
          type="button"
          className="ghost"
          disabled={!selected?.allow_upstream}
          onClick={() => assignSelection("target")}
        >
          Use as target
        </button>
      </div>
      <label>
        Source
        <select
          value={sourceId}
          onChange={(event) => {
            setSourceId(event.target.value);
            setColumns([]);
            setColumnHint(null);
          }}
          required
        >
          <option value="">Choose a source</option>
          {sources.map((resource) => (
            <option key={resource.id} value={resource.id}>
              {resourceTitle(resource.id, labels)}
            </option>
          ))}
        </select>
      </label>
      <label>
        Target
        <select value={targetId} onChange={(event) => setTargetId(event.target.value)} required>
          <option value="">Choose a target</option>
          {targets.map((resource) => (
            <option key={resource.id} value={resource.id}>
              {resourceTitle(resource.id, labels)}
            </option>
          ))}
        </select>
      </label>
      <div className="split">
        <label>
          Source table
          <input
            value={tableSource}
            onChange={(event) => {
              setTableSource(event.target.value);
              setColumns([]);
              setColumnHint(null);
            }}
            required
          />
        </label>
        <label>
          Target table
          <input value={tableTarget} onChange={(event) => setTableTarget(event.target.value)} required />
        </label>
      </div>
      {columnMessage ? <p className="hint">{columnMessage}</p> : null}
      <div className="split">
        <label>
          Pull strategy
          <select
            value={strategy}
            onChange={(event) => setStrategy(event.target.value as PullStrategy)}
          >
            {PULL_STRATEGIES.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </label>
        <label>
          Pull field
          <ColumnSelect
            value={pullField}
            columns={columns}
            onChange={setPullField}
          />
        </label>
      </div>

      <div className="rules">
        {rules.map((rule, index) => (
          <RuleEditor
            key={index}
            index={index}
            rule={rule}
            columns={columns}
            canRemove={rules.length > 1}
            onChange={(next) =>
              setRules((current) => current.map((item, itemIndex) => (itemIndex === index ? next : item)))
            }
            onRemove={() => setRules((current) => current.filter((_, itemIndex) => itemIndex !== index))}
          />
        ))}
        <button
          type="button"
          className="ghost"
          onClick={() => setRules((current) => [...current, emptyRule()])}
        >
          Add rule
        </button>
      </div>

      <button type="submit" disabled={pending || columns.length === 0}>
        {pending ? "Linking…" : "Link resources"}
      </button>
      {notice ? <p className="banner ok">{notice}</p> : null}
    </form>
  );
}

function RuleEditor({
  index,
  rule,
  columns,
  canRemove,
  onChange,
  onRemove,
}: {
  index: number;
  rule: RuleInput;
  columns: ColumnMeta[];
  canRemove: boolean;
  onChange: (rule: RuleInput) => void;
  onRemove: () => void;
}) {
  function updateField(fieldIndex: number, next: FieldInput) {
    onChange({
      ...rule,
      fields: rule.fields.map((field, index) => (index === fieldIndex ? next : field)),
    });
  }

  return (
    <fieldset className="rule-card">
      <legend>Rule {index + 1}</legend>
      <label>
        Source identifier
        <ColumnSelect
          value={rule.source_identifier}
          columns={columns}
          onChange={(source_identifier) => onChange({ ...rule, source_identifier })}
        />
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={rule.force_pull_from_start}
          onChange={(event) =>
            onChange({ ...rule, force_pull_from_start: event.target.checked })
          }
        />
        Force pull from start
      </label>
      <div className="split">
        <label>
          Source names
          <select
            value={rule.source_naming_convention}
            onChange={(event) =>
              onChange({
                ...rule,
                source_naming_convention: event.target.value as NamingConvention,
              })
            }
          >
            {NAMING_CONVENTIONS.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </label>
        <label>
          Target names
          <select
            value={rule.target_naming_convention}
            onChange={(event) =>
              onChange({
                ...rule,
                target_naming_convention: event.target.value as NamingConvention,
              })
            }
          >
            {NAMING_CONVENTIONS.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </label>
      </div>
      {rule.fields.map((field, fieldIndex) => (
        <div className="split field-row" key={fieldIndex}>
          <label>
            Field
            <ColumnSelect
              value={field.field_name}
              columns={columns}
              onChange={(field_name) => updateField(fieldIndex, { ...field, field_name })}
            />
          </label>
          <label>
            Sanitize
            <select
              value={field.sanitize_type}
              onChange={(event) =>
                updateField(fieldIndex, {
                  ...field,
                  sanitize_type: event.target.value as SanitizeType,
                })
              }
            >
              {SANITIZE_TYPES.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="ghost tiny"
            disabled={rule.fields.length === 1}
            onClick={() =>
              onChange({
                ...rule,
                fields: rule.fields.filter((_, index) => index !== fieldIndex),
              })
            }
          >
            Remove
          </button>
        </div>
      ))}
      <div className="split">
        <button
          type="button"
          className="ghost"
          onClick={() =>
            onChange({
              ...rule,
              fields: [...rule.fields, { field_name: "", sanitize_type: "name" }],
            })
          }
        >
          Add field
        </button>
        <button type="button" className="ghost" disabled={!canRemove} onClick={onRemove}>
          Remove rule
        </button>
      </div>
    </fieldset>
  );
}

function ColumnSelect({
  value,
  columns,
  onChange,
}: {
  value: string;
  columns: ColumnMeta[];
  onChange: (value: string) => void;
}) {
  const known = columns.some((column) => column.column_name === value);
  return (
    <select
      value={known ? value : ""}
      onChange={(event) => onChange(event.target.value)}
      required
      disabled={columns.length === 0}
    >
      <option value="">Choose a column</option>
      {columns.map((column) => (
        <option key={column.column_name} value={column.column_name}>
          {column.column_name} · {column.data_type}
          {column.is_primary_key ? " · primary key" : ""}
        </option>
      ))}
    </select>
  );
}

function isSafeTable(name: string): boolean {
  return name.length > 0 && name.length <= 255 && /^[A-Za-z_][A-Za-z0-9_]*$/.test(name);
}

function LinkDetail({ map }: { map: ResourceMapSummary }) {
  return (
    <div className="detail">
      <p>
        {map.pair_config.pull_batch_strategy} on {map.pair_config.pull_batch_field}
      </p>
      {map.rules.map((entry) => (
        <article key={entry.rule_id}>
          <h3>{entry.rule.source_identifier}</h3>
          <p>
            {entry.rule.source_naming_convention} to {entry.rule.target_naming_convention}
            {entry.rule.force_pull_from_start ? " · pull from start" : ""}
          </p>
          <ul>
            {entry.rule.source_sanitize_fields.map((field, index) => (
              <li key={`${entry.rule_id}-${index}`}>
                {field.field_name} · {field.sanitize_type}
              </li>
            ))}
          </ul>
        </article>
      ))}
    </div>
  );
}
