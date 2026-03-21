import { useMemo } from "react";
import i18n from "../lib/i18n";
import type { TaskDetail, TaskItemSummary } from "../lib/types";

interface Props {
  taskDetail: TaskDetail;
}

interface DagNode {
  item: TaskItemSummary;
  layer: number;
  col: number;
}

function statusColor(status: string): string {
  switch (status.toLowerCase()) {
    case "completed":
    case "succeeded":
      return "var(--success)";
    case "running":
    case "in_progress":
      return "var(--accent)";
    case "failed":
    case "error":
      return "var(--danger)";
    default:
      return "var(--text-tertiary)";
  }
}

function statusFill(status: string): string {
  switch (status.toLowerCase()) {
    case "completed":
    case "succeeded":
      return "rgba(52,199,89,0.15)";
    case "running":
    case "in_progress":
      return "rgba(0,122,255,0.12)";
    case "failed":
    case "error":
      return "rgba(255,59,48,0.12)";
    default:
      return "var(--glass-bg)";
  }
}

/**
 * Parse graph_debug effective_graph_json if available from raw TaskDetail,
 * otherwise fall back to sequential layout.
 */
function buildDag(items: TaskItemSummary[]): { nodes: DagNode[]; layers: number; maxCols: number } {
  if (items.length === 0) return { nodes: [], layers: 0, maxCols: 0 };

  // Try to detect parallelism: items sharing the same order_no are parallel.
  const byOrder = new Map<number, TaskItemSummary[]>();
  for (const item of items) {
    const group = byOrder.get(item.order_no) ?? [];
    group.push(item);
    byOrder.set(item.order_no, group);
  }

  const sortedOrders = [...byOrder.keys()].sort((a, b) => a - b);
  const nodes: DagNode[] = [];
  let maxCols = 0;

  sortedOrders.forEach((order, layerIdx) => {
    const group = byOrder.get(order)!;
    maxCols = Math.max(maxCols, group.length);
    group.forEach((item, colIdx) => {
      nodes.push({ item, layer: layerIdx, col: colIdx });
    });
  });

  return { nodes, layers: sortedOrders.length, maxCols };
}

const NODE_W = 160;
const NODE_H = 44;
const H_GAP = 24;
const V_GAP = 32;
const PAD = 16;

/** SVG-based DAG visualization with parallel branch support. */
export default function ExpertWorkflow({ taskDetail }: Props) {
  const items = taskDetail.items;

  const { nodes, layers, maxCols } = useMemo(() => buildDag(items), [items]);

  if (nodes.length === 0) {
    return <p style={{ color: "var(--text-secondary)" }}>{i18n.expertWorkflow.noSteps}</p>;
  }

  const svgW = maxCols * (NODE_W + H_GAP) - H_GAP + PAD * 2;
  const svgH = layers * (NODE_H + V_GAP) - V_GAP + PAD * 2;

  // Compute node positions.
  const byLayer = new Map<number, DagNode[]>();
  for (const n of nodes) {
    const group = byLayer.get(n.layer) ?? [];
    group.push(n);
    byLayer.set(n.layer, group);
  }

  function nodeX(node: DagNode): number {
    const group = byLayer.get(node.layer)!;
    const groupW = group.length * (NODE_W + H_GAP) - H_GAP;
    const offsetX = (svgW - PAD * 2 - groupW) / 2;
    return PAD + offsetX + node.col * (NODE_W + H_GAP);
  }

  function nodeY(node: DagNode): number {
    return PAD + node.layer * (NODE_H + V_GAP);
  }

  // Build edges: connect each layer to the next.
  const edges: { from: DagNode; to: DagNode }[] = [];
  const sortedLayers = [...byLayer.keys()].sort((a, b) => a - b);
  for (let i = 0; i < sortedLayers.length - 1; i++) {
    const curLayer = byLayer.get(sortedLayers[i])!;
    const nextLayer = byLayer.get(sortedLayers[i + 1])!;
    for (const from of curLayer) {
      for (const to of nextLayer) {
        edges.push({ from, to });
      }
    }
  }

  return (
    <div>
      <h4 style={{ marginBottom: 12, color: "var(--text-secondary)", fontSize: 13 }}>
        {i18n.expertWorkflow.stepProgress(taskDetail.finished_items, taskDetail.total_items)}
      </h4>
      <div style={{ overflowX: "auto" }}>
        <svg width={svgW} height={svgH} style={{ display: "block" }}>
          <defs>
            <marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5"
              markerWidth="6" markerHeight="6" orient="auto-start-reverse">
              <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--text-tertiary)" />
            </marker>
          </defs>

          {/* Edges */}
          {edges.map((e, i) => {
            const x1 = nodeX(e.from) + NODE_W / 2;
            const y1 = nodeY(e.from) + NODE_H;
            const x2 = nodeX(e.to) + NODE_W / 2;
            const y2 = nodeY(e.to);
            return (
              <line key={`e-${i}`} x1={x1} y1={y1} x2={x2} y2={y2}
                stroke="var(--glass-border-subtle)" strokeWidth={1.5}
                markerEnd="url(#arrow)" />
            );
          })}

          {/* Nodes */}
          {nodes.map((n) => {
            const x = nodeX(n);
            const y = nodeY(n);
            const label = n.item.qa_file_path
              ? n.item.qa_file_path.split("/").pop() ?? `Step ${n.item.order_no}`
              : `Step ${n.item.order_no}`;
            return (
              <g key={n.item.id}>
                <rect x={x} y={y} width={NODE_W} height={NODE_H}
                  rx={10} ry={10}
                  fill={statusFill(n.item.status)}
                  stroke={statusColor(n.item.status)}
                  strokeWidth={1.5} />
                <text x={x + 8} y={y + 17} fontSize={11} fill={statusColor(n.item.status)} fontWeight={600}>
                  {n.item.order_no}.
                </text>
                <text x={x + 28} y={y + 17} fontSize={11} fill="var(--text-primary)"
                  clipPath={`inset(0 0 0 0)`}>
                  {label.length > 16 ? label.slice(0, 15) + "\u2026" : label}
                </text>
                <text x={x + 8} y={y + 34} fontSize={10} fill={statusColor(n.item.status)}>
                  {n.item.status}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
