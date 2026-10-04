import { Grid, Html, OrbitControls } from "@react-three/drei";
import { Canvas, useFrame } from "@react-three/fiber";
import { useEffect, useMemo, useRef } from "react";
import * as THREE from "three";
import {
  resourceTitle,
  type ResourceLabel,
  type ResourceMapSummary,
  type ResourceSummary,
  type SanitizePreview,
} from "./api.ts";

type SceneProps = {
  resources: ResourceSummary[];
  maps: ResourceMapSummary[];
  labels: Record<string, ResourceLabel>;
  selectedResourceId: string | null;
  selectedMapId: string | null;
  transferringMapId: string | null;
  previews: Record<string, SanitizePreview>;
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
  transferringMapId,
  previews,
  onSelectResource,
  onSelectMap,
}: SceneProps) {
  const positions = useMemo(() => layout(resources), [resources]);
  const selectedMap = maps.find((map) => map.resource_map_id === selectedMapId);
  const transferringMap = maps.find((map) => map.resource_map_id === transferringMapId);

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
            receiving={transferringMap?.resource_target === resource.id}
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
            transferring={map.resource_map_id === transferringMapId}
            preview={previews[map.resource_map_id]}
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
  receiving,
  onSelect,
}: {
  resource: ResourceSummary;
  position: Vec3;
  title: string;
  selected: boolean;
  linked: boolean;
  receiving: boolean;
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
        {receiving ? (
          <ReceivingMaterial color={color} />
        ) : (
          <meshStandardMaterial
            color={color}
            roughness={0.38}
            metalness={0.18}
            emissive={color}
            emissiveIntensity={selected || linked ? 0.48 : 0.14}
          />
        )}
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
  transferring,
  preview,
  onSelect,
}: {
  start: Vec3;
  end: Vec3;
  loop: boolean;
  lift: number;
  selected: boolean;
  transferring: boolean;
  preview: SanitizePreview | undefined;
  onSelect: () => void;
}) {
  const curve = useMemo(() => linkCurve(start, end, loop, lift), [start, end, loop, lift]);
  const tableAnchor = useMemo(() => {
    const point = curve.getPoint(0.5);
    return [point.x, point.y, point.z] as Vec3;
  }, [curve]);
  const geometry = useMemo(
    () => new THREE.TubeGeometry(curve, 48, selectedWidth(lift), 6, false),
    [curve, lift],
  );
  useEffect(() => () => geometry.dispose(), [geometry]);

  return (
    <group>
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
      {transferring ? <TransferPackets curve={curve} /> : null}
      {preview ? (
        <Html position={tableAnchor} center zIndexRange={[40, 0]} style={{ pointerEvents: "none" }}>
          <div className="preview-float">
            <PreviewTable preview={preview} />
          </div>
        </Html>
      ) : null}
    </group>
  );
}

function PreviewTable({ preview }: { preview: SanitizePreview }) {
  const before = preview.examples[0]?.before ?? {};
  const after: Record<string, unknown> = { ...before };
  const changed = new Set<string>();
  for (const example of preview.examples) {
    for (const column of preview.columns) {
      if (!sameValue(example.before[column], example.after[column])) {
        after[column] = example.after[column];
        changed.add(column);
      }
    }
  }

  return (
    <div
      className="preview-card"
      onPointerDown={(event) => event.stopPropagation()}
      onWheel={(event) => event.stopPropagation()}
    >
      <table className="preview-table">
        <thead>
          <tr>
            <th>Column</th>
            <th>Before</th>
            <th>After</th>
          </tr>
        </thead>
        <tbody>
          {preview.columns.map((column) => (
            <tr key={column} className={changed.has(column) ? "changed" : undefined}>
              <th>{column}</th>
              <td>{formatValue(before[column])}</td>
              <td>{formatValue(after[column])}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function sameValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function formatValue(value: unknown): string {
  if (value == null) return "null";
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  return JSON.stringify(value);
}

const PACKET_COUNT = 7;

function TransferPackets({ curve }: { curve: THREE.QuadraticBezierCurve3 }) {
  const mesh = useRef<THREE.InstancedMesh>(null);
  const dummy = useMemo(() => new THREE.Object3D(), []);

  useFrame(({ clock }) => {
    const instanced = mesh.current;
    if (!instanced) return;
    const time = clock.getElapsedTime();
    const canTravel = curve.getLength() > 1e-4;
    for (let index = 0; index < PACKET_COUNT; index += 1) {
      const phase = (time * 0.45 + index / PACKET_COUNT) % 1;
      dummy.position.copy(canTravel ? curve.getPointAt(phase) : curve.getPoint(0));
      const fade = Math.sin(phase * Math.PI);
      dummy.scale.setScalar(0.35 + fade * 0.9);
      dummy.updateMatrix();
      instanced.setMatrixAt(index, dummy.matrix);
    }
    instanced.instanceMatrix.needsUpdate = true;
  });

  return (
    <instancedMesh ref={mesh} args={[undefined, undefined, PACKET_COUNT]} frustumCulled={false}>
      <sphereGeometry args={[0.07, 10, 10]} />
      <meshStandardMaterial color="#f4e6c4" emissive="#e7c27a" emissiveIntensity={0.85} />
    </instancedMesh>
  );
}

function ReceivingMaterial({ color }: { color: string }) {
  const material = useRef<THREE.MeshStandardMaterial>(null);
  useFrame(({ clock }) => {
    if (!material.current) return;
    material.current.emissiveIntensity = 0.55 + Math.sin(clock.getElapsedTime() * 6) * 0.35;
  });
  return (
    <meshStandardMaterial
      ref={material}
      color={color}
      roughness={0.38}
      metalness={0.18}
      emissive={color}
      emissiveIntensity={0.7}
    />
  );
}

function linkCurve(
  start: Vec3,
  end: Vec3,
  loop: boolean,
  lift: number,
): THREE.QuadraticBezierCurve3 {
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

  return new THREE.QuadraticBezierCurve3(from, control, to);
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
