import { useState } from "react";
import i18n from "../lib/i18n";
import type { TaskDetail } from "../lib/types";

interface Props {
  taskDetail: TaskDetail;
}

export default function ExpertRawData({ taskDetail }: Props) {
  const [copied, setCopied] = useState(false);
  const json = JSON.stringify(taskDetail, null, 2);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(json);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", marginBottom: 8 }}>
        <h4 style={{ flex: 1, color: "var(--text-secondary)", fontSize: 13 }}>
          {i18n.expertRawData.title}
        </h4>
        <button className="btn btn-ghost" onClick={handleCopy} style={{ fontSize: 12 }}>
          {copied ? i18n.common.copied : i18n.common.copy}
        </button>
      </div>
      <div
        style={{
          background: "var(--bg-secondary)",
          borderRadius: 12,
          padding: 12,
          fontFamily: "monospace",
          fontSize: 12,
          lineHeight: 1.5,
          whiteSpace: "pre-wrap",
          wordBreak: "break-word",
          maxHeight: 500,
          overflowY: "auto",
          color: "var(--text-primary)",
        }}
      >
        {json}
      </div>
    </div>
  );
}
