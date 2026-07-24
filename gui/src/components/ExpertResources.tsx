import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useRole } from "../hooks/useRole";
import i18n from "../lib/i18n";
import type {
  ResourceApplyResult,
  ResourceCatalogResult,
  ResourceDescribeResult,
  ResourceSummary,
} from "../lib/types";
import ReviewedActionDialog from "./ReviewedActionDialog";

const RESOURCE_KINDS = [
  { query: "workspaces", label: "Workspaces" },
  { query: "workflows", label: "Workflows" },
  { query: "agents", label: "Agents" },
  { query: "steptemplates", label: "Step Templates" },
  { query: "executionprofiles", label: "Execution Profiles" },
] as const;

function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

function resourcePath(resource: ResourceSummary): string {
  return `${resource.kind.toLowerCase()}/${resource.name}`;
}

function mutationKey(resource: ResourceSummary): string {
  return `expert-resource-${resource.kind.toLowerCase()}-${resource.name}-${Date.now()}`;
}

export default function ExpertResources() {
  const [kind, setKind] = useState("workspaces");
  const [resources, setResources] = useState<ResourceSummary[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [detail, setDetail] = useState<ResourceDescribeResult | null>(null);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [confirmApply, setConfirmApply] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [applyMsg, setApplyMsg] = useState<string | null>(null);
  const [copyMsg, setCopyMsg] = useState<string | null>(null);
  const { canAccess } = useRole();
  const rowRefs = useRef(new Map<string, HTMLButtonElement>());
  const lastSelectedRef = useRef<string | null>(null);
  const detailHeadingRef = useRef<HTMLHeadingElement>(null);
  const mutationKeyRef = useRef<string | null>(null);

  const loadResources = useCallback(async (resourceType: string, cursor?: string) => {
    cursor ? setLoadingMore(true) : setLoading(true);
    if (!cursor) {
      setResources([]);
      setNextCursor(null);
      setDetail(null);
      setEditing(false);
    }
    setError(null);
    setApplyMsg(null);
    setCopyMsg(null);
    try {
      const result = await invoke<ResourceCatalogResult>("resource_list", {
        resourceType,
        projectId: null,
        cursor: cursor ?? null,
        limit: 100,
      });
      setResources((current) => cursor ? [...current, ...result.resources] : result.resources);
      setNextCursor(result.next_cursor);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoading(false);
      setLoadingMore(false);
    }
  }, []);

  useEffect(() => {
    void loadResources(kind);
  }, [kind, loadResources]);

  useEffect(() => {
    if (detail) {
      requestAnimationFrame(() => detailHeadingRef.current?.focus());
    } else if (!loading && lastSelectedRef.current) {
      requestAnimationFrame(() => rowRefs.current.get(lastSelectedRef.current!)?.focus());
    }
  }, [detail, loading]);

  const describeResource = async (
    resource: ResourceSummary,
    options: { preserveDraft?: boolean; preserveFeedback?: boolean } = {},
  ) => {
    lastSelectedRef.current = `${resource.kind}/${resource.name}`;
    setLoading(true);
    setError(null);
    setConflict(false);
    if (!options.preserveFeedback) {
      setApplyMsg(null);
      setCopyMsg(null);
    }
    try {
      const result = await invoke<ResourceDescribeResult>("resource_describe", {
        resource: resourcePath(resource),
        outputFormat: "yaml",
        projectId: resource.project_id,
      });
      setDetail(result);
      if (!options.preserveDraft) setEditContent(result.content);
      return result;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    } finally {
      setLoading(false);
    }
  };

  const selectedResource = detail?.resource;

  const beginEditing = () => {
    if (!selectedResource) return;
    mutationKeyRef.current = mutationKey(selectedResource);
    setEditContent(detail.content);
    setEditing(true);
    setError(null);
    setConflict(false);
    setApplyMsg(null);
  };

  const handleApply = async (reason: string) => {
    if (!selectedResource) return;
    setConfirmApply(false);
    setApplying(true);
    setError(null);
    setConflict(false);
    setApplyMsg(null);
    try {
      const result = await invoke<ResourceApplyResult>("resource_apply", {
        content: editContent,
        project_id: selectedResource.project_id,
        expected_revision: selectedResource.revision,
        require_absent: false,
        reason,
        idempotency_key: mutationKeyRef.current ?? mutationKey(selectedResource),
      });
      const refreshed = await describeResource(selectedResource, { preserveFeedback: true });
      if (refreshed) {
        setEditing(false);
        mutationKeyRef.current = null;
        setApplyMsg(
          result.request_id
            ? `${result.message} · ${i18n.expertResources.auditRequest}: ${result.request_id}`
            : result.message,
        );
      }
    } catch (cause) {
      const message = errorMessage(cause);
      setError(message);
      setConflict(message.includes("重新加载") || message.toLowerCase().includes("refresh"));
    } finally {
      setApplying(false);
    }
  };

  const reloadAfterConflict = async () => {
    if (!selectedResource) return;
    const refreshed = await describeResource(selectedResource, {
      preserveDraft: true,
      preserveFeedback: true,
    });
    if (refreshed) {
      setEditing(true);
      setError(i18n.expertResources.authorityReloaded);
      mutationKeyRef.current = mutationKey(refreshed.resource ?? selectedResource);
    }
  };

  const copyContent = async (content: string, message: string) => {
    await navigator.clipboard.writeText(content);
    setCopyMsg(message);
  };

  return (
    <div className="expert-resources">
      <div className="resource-kind-tabs" role="tablist" aria-label={i18n.expertResources.kindFilter}>
        {RESOURCE_KINDS.map((item) => (
          <button
            key={item.query}
            type="button"
            role="tab"
            aria-selected={kind === item.query}
            className={`btn ${kind === item.query ? "btn-primary" : "btn-ghost"}`}
            onClick={() => setKind(item.query)}
          >
            {item.label}
          </button>
        ))}
      </div>

      {error && <p role="alert" className="resource-feedback resource-error">{error}</p>}
      {applyMsg && <p role="status" className="resource-feedback resource-success">{applyMsg}</p>}
      {copyMsg && <p role="status" className="resource-feedback">{copyMsg}</p>}

      {!detail && (
        <section aria-label={i18n.expertResources.catalog}>
          {loading && <p role="status" className="empty-state">{i18n.expertResources.loading}</p>}
          {!loading && resources.length === 0 && !error && (
            <p className="empty-state">{i18n.expertResources.empty}</p>
          )}
          {!loading && resources.length > 0 && (
            <ul className="resource-catalog">
              {resources.map((resource) => {
                const key = `${resource.kind}/${resource.name}`;
                return (
                  <li key={`${resource.project_id}/${key}`}>
                    <button
                      ref={(element) => {
                        if (element) rowRefs.current.set(key, element);
                        else rowRefs.current.delete(key);
                      }}
                      type="button"
                      className="resource-row"
                      aria-label={`${i18n.expertResources.open} ${resource.kind} ${resource.name}`}
                      onClick={() => void describeResource(resource)}
                    >
                      <span>
                        <strong>{resource.name}</strong>
                        <small>{resource.kind} · {resource.project_id}</small>
                      </span>
                      <code>{resource.revision.slice(0, 10)}</code>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
          {nextCursor && (
            <button
              type="button"
              className="btn btn-secondary"
              disabled={loadingMore}
              onClick={() => void loadResources(kind, nextCursor)}
            >
              {loadingMore ? i18n.expertResources.loading : i18n.expertResources.loadMore}
            </button>
          )}
        </section>
      )}

      {detail && selectedResource && (
        <section className="resource-detail" aria-labelledby="resource-detail-title">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => {
              setDetail(null);
              setEditing(false);
              setError(null);
              setApplyMsg(null);
              setCopyMsg(null);
            }}
          >
            {i18n.expertResources.backToList}
          </button>
          <header className="pane-heading">
            <div>
              <h3 id="resource-detail-title" ref={detailHeadingRef} tabIndex={-1}>
                {selectedResource.kind}/{selectedResource.name}
              </h3>
              <small>
                {selectedResource.project_id} · revision {selectedResource.revision.slice(0, 12)}
              </small>
            </div>
          </header>

          {!editing && <pre className="resource-manifest">{detail.content}</pre>}

          {editing && (
            <div className="resource-editor">
              <label htmlFor="resource-manifest-editor">{i18n.expertResources.manifest}</label>
              <textarea
                id="resource-manifest-editor"
                autoFocus
                value={editContent}
                onChange={(event) => setEditContent(event.target.value)}
              />
              <div className="decision-actions">
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={applying}
                  onClick={() => setConfirmApply(true)}
                >
                  {applying ? i18n.expertResources.applying : i18n.common.apply}
                </button>
                <button
                  type="button"
                  className="btn btn-ghost"
                  disabled={applying}
                  onClick={() => {
                    setEditing(false);
                    setEditContent(detail.content);
                    setError(null);
                    setConflict(false);
                  }}
                >
                  {i18n.common.cancel}
                </button>
                {conflict && (
                  <button
                    type="button"
                    className="btn btn-secondary"
                    onClick={() => void reloadAfterConflict()}
                  >
                    {i18n.expertResources.reloadAuthority}
                  </button>
                )}
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() => void copyContent(editContent, i18n.expertResources.draftCopied)}
                >
                  {i18n.expertResources.copyDraft}
                </button>
              </div>
            </div>
          )}

          {!editing && (
            <div className="decision-actions">
              {canAccess("operator") && (
                <button type="button" className="btn btn-secondary" onClick={beginEditing}>
                  {i18n.common.edit}
                </button>
              )}
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => void copyContent(detail.content, i18n.expertResources.manifestCopied)}
              >
                {i18n.common.copy}
              </button>
            </div>
          )}
        </section>
      )}

      <ReviewedActionDialog
        open={confirmApply}
        title={i18n.expertResources.confirmTitle}
        description={
          selectedResource
            ? `${selectedResource.kind}/${selectedResource.name} · ${selectedResource.project_id}. ${i18n.expertResources.confirmDescription}`
            : i18n.expertResources.confirmDescription
        }
        confirmLabel={i18n.expertResources.confirmApply}
        onConfirm={(reason) => void handleApply(reason)}
        onCancel={() => setConfirmApply(false)}
      />
    </div>
  );
}
