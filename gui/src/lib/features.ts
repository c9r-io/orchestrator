export type ConsoleFeature = "attention" | "processes" | "sessions" | "sources" | "system";

const envByFeature: Record<ConsoleFeature, string | undefined> = {
  attention: import.meta.env.VITE_CONSOLE_ATTENTION,
  processes: import.meta.env.VITE_CONSOLE_PROCESSES,
  sessions: import.meta.env.VITE_CONSOLE_SESSIONS,
  sources: import.meta.env.VITE_CONSOLE_SOURCES,
  system: import.meta.env.VITE_CONSOLE_SYSTEM,
};

export function featureEnabled(feature: ConsoleFeature): boolean {
  return envByFeature[feature]?.toLowerCase() !== "false";
}
