import { Grid, Html, OrbitControls } from "@react-three/drei";
import { Canvas } from "@react-three/fiber";
import { useEffect, useMemo } from "react";
import * as THREE from "three";
import {
  resourceTitle,
  type ResourceLabel,
  type ResourceMapSummary,
  type ResourceSummary,
} from "./api.ts";

type SceneProps = {
  resources: ResourceSummary[];
  maps: ResourceMapSummary[];
  labels: Record<string, ResourceLabel>;
  selectedResourceId: string | null;
  selectedMapId: string | null;
  onSelectResource: (id: string) => void;
  onSelectMap: (id: string) => void;
};

type Vec3 = [number, number, number];

export function Scene(props: SceneProps) {
  return (
    <Canvas camera={{ position: [1.6, 6.2, 11], fov: 42 }}>
      <color attach="background" args={["#12161c"]} />
      <fog attach="fog" args={["#12161c", 16, 38]} />
      <ambientLight intensity={0.5} />
      <directionalLight position={[7, 11, 5]} intensity={1.5} />
      <directionalLight position={[-6, 4, -4]} intensity={0.4} color="#9fd4d0" />
      <Grid
        position={[1.6, -0.75, 0]}
        args={[30, 30]}
        cellSize={0.8}
        cellThickness={0.5}
        cellColor="#2c3440"
        sectionSize={4}
        sectionThickness={1}
        sectionColor="#3c4654"
        fadeDistance={26}
        fadeStrength={1.5}
        infiniteGrid
      />
      <ResourceField {...props} />
      <OrbitControls
        makeDefault
        enableDamping
        target={[1.6, 0, 0]}
        maxPolarAngle={Math.PI / 2.08}
      />
    </Canvas>
  );
}

function ResourceField({
  resources,
  maps,
  labels,
  selectedResourceId,
  selectedMapId,
  onSelectResource,
  onSelectMap,
}: SceneProps) {
  const positions = useMemo(() => layout(resources), [resources]);
  const selectedMap = maps.find((map) => map.resource_map_id === selectedMapId);

  return (
    <group>
      {resources.map((resource) => {
        const position = positions.get(resource.id);
        if (!position) return null;
        const linked =
          selectedMap?.resource_source === resource.id ||
          selectedMap?.resource_target === resource.id;
        return (
          <ResourceNode
            key={resource.id}
            resource={resource}
            position={position}
            title={resourceTitle(resource.id, labels)}
            selected={resource.id === selectedResourceId}
            linked={linked}
            onSelect={() => onSelectResource(resource.id)}
          />
        );
      })}
      {maps.map((map, index) => {
        const start = positions.get(map.resource_source);
        const end = positions.get(map.resource_target);
        if (!start || !end) return null;
        return (
          <LinkArc
            key={map.resource_map_id}
            start={start}
            end={end}
            loop={map.resource_source === map.resource_target}
            lift={0.85 + (index % 4) * 0.28}
            selected={map.resource_map_id === selectedMapId}
            onSelect={() => onSelectMap(map.resource_map_id)}
          />
        );
      })}
    </group>
  );
}

function layout(resources: ResourceSummary[]): Map<string, Vec3> {
  const positions = new Map<string, Vec3>();
  const count = Math.max(resources.length, 1);
  const radius = Math.max(3.4, resources.length * 1.15);
  resources.forEach((resource, index) => {
    const angle = (index / count) * Math.PI * 2 - Math.PI / 2;
    positions.set(resource.id, [
      Math.cos(angle) * radius + 1.6,
      0,
      Math.sin(angle) * radius,
    ]);
  });
  return positions;
}

function ResourceNode({
  resource,
  position,
  title,
  selected,
  linked,
  onSelect,
}: {
  resource: ResourceSummary;
  position: Vec3;
  title: string;
  selected: boolean;
  linked: boolean;
  onSelect: () => void;
}) {
  const color = nodeColor(resource);
  return (
    <group position={position}>
      <mesh
        scale={selected ? 1.18 : 1}
        onClick={(event) => {
          event.stopPropagation();
          onSelect();
        }}
        onPointerOver={(event) => {
          event.stopPropagation();
          document.body.style.cursor = "pointer";
        }}
        onPointerOut={() => {
          document.body.style.cursor = "auto";
        }}
      >
        <icosahedronGeometry args={[0.55, 1]} />
        <meshStandardMaterial
          color={color}
          roughness={0.38}
          metalness={0.18}
          emissive={color}
          emissiveIntensity={selected || linked ? 0.48 : 0.14}
        />
      </mesh>
      <Html center distanceFactor={9} position={[0, 0.95, 0]} zIndexRange={[20, 0]}>
        <div className="node-label">
          <strong>{title}</strong>
          <span>{flagText(resource)}</span>
        </div>
      </Html>
    </group>
  );
}

function LinkArc({
  start,
  end,
  loop,
  lift,
  selected,
  onSelect,
}: {
  start: Vec3;
  end: Vec3;
  loop: boolean;
  lift: number;
  selected: boolean;
  onSelect: () => void;
}) {
  const geometry = useMemo(
    () => buildLinkGeometry(start, end, loop, lift),
    [start, end, loop, lift],
  );
  useEffect(() => () => geometry.dispose(), [geometry]);

  return (
    <mesh
      geometry={geometry}
      onClick={(event) => {
        event.stopPropagation();
        onSelect();
      }}
      onPointerOver={(event) => {
        event.stopPropagation();
        document.body.style.cursor = "pointer";
      }}
      onPointerOut={() => {
        document.body.style.cursor = "auto";
      }}
    >
      <meshStandardMaterial
        color={selected ? "#f4e6c4" : "#c9844a"}
        emissive={selected ? "#e7c27a" : "#5c3318"}
        emissiveIntensity={selected ? 0.6 : 0.28}
        roughness={0.42}
      />
    </mesh>
  );
}

function buildLinkGeometry(
  start: Vec3,
  end: Vec3,
  loop: boolean,
  lift: number,
): THREE.TubeGeometry {
  const origin = new THREE.Vector3(...start);
  const destination = new THREE.Vector3(...end);
  const radius = 0.62;
  let from: THREE.Vector3;
  let to: THREE.Vector3;
  let control: THREE.Vector3;

  if (loop) {
    from = origin.clone().add(new THREE.Vector3(0.4, radius, 0));
    to = origin.clone().add(new THREE.Vector3(-0.4, radius * 0.45, 0.08));
    control = origin.clone().add(new THREE.Vector3(0, radius + lift + 0.8, 1.15));
  } else {
    const direction = destination.clone().sub(origin);
    const normal =
      direction.lengthSq() < 1e-6
        ? new THREE.Vector3(0, 1, 0)
        : direction.normalize();
    from = origin.clone().add(normal.clone().multiplyScalar(radius));
    to = destination.clone().add(normal.clone().multiplyScalar(-radius));
    control = from.clone().add(to).multiplyScalar(0.5);
    control.y += lift;
  }

  return new THREE.TubeGeometry(
    new THREE.QuadraticBezierCurve3(from, control, to),
    48,
    selectedWidth(lift),
    6,
    false,
  );
}

function selectedWidth(lift: number): number {
  return 0.028 + (lift % 0.2) * 0.01;
}

function nodeColor(resource: ResourceSummary): string {
  if (resource.allow_downstream && resource.allow_upstream) return "#e7c27a";
  if (resource.allow_downstream) return "#e08a45";
  return resource.allow_upstream ? "#6eb8b4" : "#6a7380";
}

function flagText(resource: ResourceSummary): string {
  const parts = [
    resource.allow_downstream ? "source" : null,
    resource.allow_upstream ? "target" : null,
  ].filter((part) => part !== null);
  return parts.length > 0 ? parts.join(" · ") : "unused";
}
