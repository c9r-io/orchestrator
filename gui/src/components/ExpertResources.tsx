import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import type { ResourceResult } from "../lib/types";

const RESOURCE_KINDS = ["workspaces", "workflows", "agents", "steptemplates", "executionprofiles"];

export default function ExpertResources() {
  const [kind, setKind] = useState("workspaces");
  const [content, setContent] = useState<string | null>(null);
  const [selectedResource, setSelectedResource] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [applyMsg, setApplyMsg] = useState<string | null>(null);
  const { canAccess } = useRole();

  const loadResources = async (k: string) => {
    setKind(k);
    setSelectedResource(null);
    setContent(null);
    setEditing(false);
    setError(null);
    setApplyMsg(null);
    try {
      const result = await invoke<ResourceResult>("resource_get", {
        resource: k,
        outputFormat: "yaml",
      });
      setContent(result.content);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  const describeResource = async (resource: string) => {
    setSelectedResource(resource);
    setEditing(false);
    setError(null);
    setApplyMsg(null);
    try {
      const result = await invoke<ResourceResult>("resource_describe", {
        resource,
        outputFormat: "yaml",
      });
      setContent(result.content);
      setEditContent(result.content);
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  const handleApply = async () => {
    setError(null);
    setApplyMsg(null);
    try {
      const msg = await invoke<string>("resource_apply", { content: editContent });
      setApplyMsg(msg);
      setEditing(false);
      if (selectedResource) {
        await describeResource(selectedResource);
      }
    } catch (e) {
      setError(typeof e === "string" ? e : String(e));
    }
  };

  return (
    <div>
      {/* Kind filter */}
      <div style={{ display: "flex", gap: 4, marginBottom: 12, flexWrap: "wrap" }}>
        {RESOURCE_KINDS.map((k) => (
          <button
            key={k}
            className={`btn ${kind === k && !selectedResource ? "btn-primary" : "btn-ghost"}`}
            onClick={() => loadResources(k)}
            style={{ fontSize: 12, padding: "4px 10px" }}
          >
            {k}
          </button>
        ))}
      </div>

      {selectedResource && (
        <button
          className="btn btn-ghost"
          onClick={() => { setSelectedResource(null); loadResources(kind); }}
          style={{ marginBottom: 8, fontSize: 13 }}
        >
          &larr; 返回列表
        </button>
      )}

      {error && <p style={{ color: "var(--danger)", fontSize: 13 }}>{error}</p>}
      {applyMsg && <p style={{ color: "var(--success)", fontSize: 13 }}>{applyMsg}</p>}

      {/* Content display */}
      {content && !editing && (
        <div
          style={{
            background: "var(--bg-secondary)",
            borderRadius: 12,
            padding: 12,
            fontFamily: "monospace",
            fontSize: 13,
            lineHeight: 1.6,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            maxHeight: 400,
            overflowY: "auto",
          }}
        >
          {content}
        </div>
      )}

      {/* Edit mode */}
      {editing && (
        <div>
          <textarea
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            style={{
              width: "100%",
              minHeight: 300,
              background: "var(--bg-secondary)",
              border: "1px solid var(--glass-border-subtle)",
              borderRadius: 12,
              padding: 12,
              fontFamily: "monospace",
              fontSize: 13,
              color: "var(--text-primary)",
              resize: "vertical",
              outline: "none",
            }}
          />
          <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
            <button className="btn btn-primary" onClick={handleApply}>
              应用
            </button>
            <button className="btn btn-ghost" onClick={() => setEditing(false)}>
              取消
            </button>
          </div>
        </div>
      )}

      {/* Edit button */}
      {selectedResource && !editing && canAccess("operator") && (
        <div style={{ marginTop: 8, display: "flex", gap: 8 }}>
          <button className="btn btn-secondary" onClick={() => setEditing(true)}>
            编辑
          </button>
          <button
            className="btn btn-ghost"
            onClick={() => navigator.clipboard.writeText(content ?? "")}
          >
            复制
          </button>
        </div>
      )}

      {/* Clickable resource list hint */}
      {!selectedResource && content && (
        <p style={{ color: "var(--text-tertiary)", fontSize: 12, marginTop: 8 }}>
          使用 resource_describe 查看详情：在上方搜索 "kind/name" 格式
        </p>
      )}
    </div>
  );
}
