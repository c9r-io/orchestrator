import { useState } from "react";
import ExpertWorkflow from "./ExpertWorkflow";
import ExpertResources from "./ExpertResources";
import ExpertAgents from "./ExpertAgents";
import ExpertStore from "./ExpertStore";
import ExpertSystem from "./ExpertSystem";
import ExpertTrigger from "./ExpertTrigger";
import ExpertSecret from "./ExpertSecret";
import ExpertRawData from "./ExpertRawData";
import i18n from "../lib/i18n";
import type { TaskDetail } from "../lib/types";

interface Props {
  taskDetail: TaskDetail;
}

type ExpertTab = "workflow" | "resources" | "agents" | "store" | "system" | "trigger" | "secret" | "raw";

const TABS: { key: ExpertTab; label: string }[] = [
  { key: "workflow", label: i18n.expert.workflow },
  { key: "resources", label: i18n.expert.resources },
  { key: "agents", label: i18n.expert.agents },
  { key: "store", label: i18n.expert.store },
  { key: "system", label: i18n.expert.system },
  { key: "trigger", label: i18n.expert.trigger },
  { key: "secret", label: i18n.expert.secret },
  { key: "raw", label: i18n.expert.rawData },
];

export default function ExpertPanel({ taskDetail }: Props) {
  const [tab, setTab] = useState<ExpertTab>("workflow");

  return (
    <div className="liquid-glass" style={{ marginTop: 16 }}>
      <nav style={{ display: "flex", gap: 4, marginBottom: 16, flexWrap: "wrap" }} aria-label={i18n.expert.navLabel}>
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
