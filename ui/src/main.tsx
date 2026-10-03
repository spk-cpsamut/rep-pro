import { StrictMode, useCallback, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  listResourceMaps,
  listResources,
  readLabels,
  type ResourceLabel,
  type ResourceMapSummary,
  type ResourceSummary,
} from "./api.ts";
import { Panel } from "./panel.tsx";
import { Scene } from "./scene.tsx";
import "./styles.css";

function App() {
  const [resources, setResources] = useState<ResourceSummary[]>([]);
  const [maps, setMaps] = useState<ResourceMapSummary[]>([]);
  const [labels, setLabels] = useState(readLabels);
  const [selectedResourceId, setSelectedResourceId] = useState<string | null>(null);
  const [selectedMapId, setSelectedMapId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    const [nextResources, nextMaps] = await Promise.all([
      listResources(),
      listResourceMaps(),
    ]);
    setResources(nextResources);
    setMaps(nextMaps);
    setError(null);
  }, []);

  useEffect(() => {
    reload().catch((err: unknown) => {
      setError(err instanceof Error ? err.message : "Could not load resources");
    });
  }, [reload]);

  return (
    <div className="app">
      <Scene
        resources={resources}
        maps={maps}
        labels={labels}
        selectedResourceId={selectedResourceId}
        selectedMapId={selectedMapId}
        onSelectResource={setSelectedResourceId}
        onSelectMap={setSelectedMapId}
      />
      <Panel
        resources={resources}
        maps={maps}
        labels={labels}
        selectedResourceId={selectedResourceId}
        selectedMapId={selectedMapId}
        error={error}
        onSelectMap={setSelectedMapId}
        onCreated={(id, label: ResourceLabel) => {
          setLabels((current) => ({ ...current, [id]: label }));
          setSelectedResourceId(id);
          reload().catch((err: unknown) => {
            setError(err instanceof Error ? err.message : "Could not load resources");
          });
        }}
        onLinked={() => {
          reload().catch((err: unknown) => {
            setError(err instanceof Error ? err.message : "Could not load resources");
          });
        }}
        onError={setError}
      />
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
