import type { SourceAutomationBinding, SourceAutomationTemplate } from "../../lib/types";

export function templateManifest(value: Omit<SourceAutomationTemplate, "revision">): string {
  return JSON.stringify({ apiVersion: "orchestrator.dev/v2", kind: "SourceTaskTemplate", metadata: { name: value.name }, spec: {
    skill: { name: value.skill_name, invocation: value.skill_invocation, args: value.skill_args },
    action: { workflow: value.workflow, workspace: value.workspace, start: value.start, initial_vars: value.initial_vars },
    goalTemplate: value.goal_template, allowedVariables: value.allowed_variables,
  } }, null, 2);
}

export function bindingManifest(value: Omit<SourceAutomationBinding, "revision" | "installation_id">): string {
  return JSON.stringify({ apiVersion: "orchestrator.dev/v2", kind: "SourceTaskBinding", metadata: { name: value.name }, spec: {
    triggerRef: value.trigger_ref,
    match: { eventKind: "reaction_added", reaction: value.reaction, targetKind: "message", ...(value.all_channels ? { allChannels: true } : { channels: value.channels }) },
    templateRef: value.template_ref, allowedActorRoles: value.allowed_actor_roles, suspend: value.suspended,
  } }, null, 2);
}

export const splitList = (value: string) => value.split(",").map((item) => item.trim()).filter(Boolean);
