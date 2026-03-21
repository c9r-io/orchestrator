import { createContext, useContext } from "react";
import type { Role } from "../lib/types";

const ROLE_HIERARCHY: Record<Role, number> = {
  read_only: 1,
  operator: 2,
  admin: 3,
};

export interface RoleContextValue {
  role: Role | null;
  canAccess: (required: Role) => boolean;
}

export const RoleContext = createContext<RoleContextValue>({
  role: null,
  canAccess: () => false,
});

export function useRole(): RoleContextValue {
  return useContext(RoleContext);
}

/** Check if `current` role has at least `required` privilege. */
export function hasAccess(current: Role | null, required: Role): boolean {
  if (!current) return false;
  return ROLE_HIERARCHY[current] >= ROLE_HIERARCHY[required];
}
