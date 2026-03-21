import { useState } from "react";
import ExpertWorkflow from "./ExpertWorkflow";
import ExpertResources from "./ExpertResources";
import ExpertAgents from "./ExpertAgents";
import ExpertStore from "./ExpertStore";
import ExpertSystem from "./ExpertSystem";
import ExpertTrigger from "./ExpertTrigger";
import ExpertSecret from "./ExpertSecret";
import ExpertRawData from "./ExpertRawData";
import type { TaskDetail } from "../lib/types";

interface Props {
  taskDetail: TaskDetail;
}

type ExpertTab = "workflow" | "resources" | "agents" | "store" | "system" | "trigger" | "secret" | "raw";

const TABS: { key: ExpertTab; label: string }[] = [
  { key: "workflow", label: "工作流" },
  { key: "resources", label: "资源" },
  { key: "agents", label: "Agent" },
  { key: "store", label: "Store" },
  { key: "system", label: "系统" },
  { key: "trigger", label: "触发器" },
  { key: "secret", label: "密钥" },
  { key: "raw", label: "原始数据" },
];

export default function ExpertPanel({ taskDetail }: Props) {
  const [tab, setTab] = useState<ExpertTab>("workflow");

  return (
    <div className="liquid-glass" style={{ marginTop: 16 }}>
      <nav style={{ display: "flex", gap: 4, marginBottom: 16, flexWrap: "wrap" }} aria-label="专家模式导航">
        {TABS.map((t) => (
          <button
            key={t.key}
            className={`btn ${tab === t.key ? "btn-primary" : "btn-ghost"}`}
            onClick={() => setTab(t.key)}
            style={{ fontSize: 13, padding: "4px 12px" }}
            aria-current={tab === t.key ? "page" : undefined}
          >
            {t.label}
          </button>
        ))}
      </nav>

      {tab === "workflow" && <ExpertWorkflow taskDetail={taskDetail} />}
      {tab === "resources" && <ExpertResources />}
      {tab === "agents" && <ExpertAgents />}
      {tab === "store" && <ExpertStore />}
      {tab === "system" && <ExpertSystem />}
      {tab === "trigger" && <ExpertTrigger />}
      {tab === "secret" && <ExpertSecret />}
      {tab === "raw" && <ExpertRawData taskDetail={taskDetail} />}
    </div>
  );
}
