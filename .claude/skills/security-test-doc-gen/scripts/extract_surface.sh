#!/usr/bin/env bash
set -euo pipefail

# Best-effort extraction of a project's externally reachable "security surface":
# - HTTP routes (heuristics for Rust/axum style code)
# - gRPC services/methods (from .proto files)
# - UI routes (heuristics for React Router usage)
#
# Output is intended to be used as an input when tailoring docs/security/**.

ROOT="${ROOT:-$(pwd)}"
OUT_DIR="${OUT_DIR:-docs/security/_surface}"

# References live next to this script; used to init high-standard surface docs.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REF_DIR="$(cd "$SCRIPT_DIR/../references" && pwd)"

# Optional overrides (comma-separated dir names, relative to $ROOT):
# - CORE_DIRS: backend directories to scan for HTTP routes and protos
# - PORTAL_DIRS: frontend directories to scan for UI routes
CORE_DIRS="${CORE_DIRS:-}"
PORTAL_DIRS="${PORTAL_DIRS:-}"

cd "$ROOT"
mkdir -p "$OUT_DIR"

have_rg() { command -v rg >/dev/null 2>&1; }

ts() { date +"%Y-%m-%d %H:%M:%S"; }

split_csv() {
  # Prints items one per line
  local s="${1:-}"
  if [ -z "$s" ]; then
    return 0
  fi
  echo "$s" | tr ',' '\n' | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' | rg -v '^$' || true
}

pick_dirs() {
  # Usage: pick_dirs "csv_override" "candidate1" "candidate2" ...
  local override_csv="${1:-}"; shift || true
  local -a picked=()
  local d

  if [ -n "$override_csv" ]; then
    while IFS= read -r d; do
      [ -d "$d" ] && picked+=("$d")
    done < <(split_csv "$override_csv")
  else
    for d in "$@"; do
      [ -d "$d" ] && picked+=("$d")
    done
  fi

  printf "%s\n" "${picked[@]:-}"
}

echo "# security surface extract" >"$OUT_DIR/README.txt"
echo "generated_at=$(ts)" >>"$OUT_DIR/README.txt"
echo "root=$ROOT" >>"$OUT_DIR/README.txt"
echo >>"$OUT_DIR/README.txt"

HTTP_OUT="$OUT_DIR/http_routes.txt"
GRPC_OUT="$OUT_DIR/grpc_services.txt"
UI_OUT="$OUT_DIR/ui_routes.txt"
OPENAPI_OUT="$OUT_DIR/openapi_paths.txt"

: >"$HTTP_OUT"
: >"$GRPC_OUT"
: >"$UI_OUT"
: >"$OPENAPI_OUT"

CORE_SCAN_DIRS=()
while IFS= read -r d; do CORE_SCAN_DIRS+=("$d"); done < <(pick_dirs "$CORE_DIRS" core auth9-core backend server services api)

PORTAL_SCAN_DIRS=()
while IFS= read -r d; do PORTAL_SCAN_DIRS+=("$d"); done < <(pick_dirs "$PORTAL_DIRS" portal auth9-portal frontend web ui)

echo "==> extracting HTTP routes (heuristics)..."
if [ "${#CORE_SCAN_DIRS[@]}" -gt 0 ] && have_rg; then
  # axum common patterns:
  # - .route("/path", get(handler))
  # - .route("/path", routing::post(handler))
  # - .route("/path", get(handler).post(handler2))
  #
  # We extract a best-effort list of (METHOD, PATH) with file:line context.
  rg -n --no-heading --pcre2 \
    '\.route\(\s*"([^"]+)"\s*,\s*(?:routing::)?(get|post|put|delete|patch|head|options)\b' \
    "${CORE_SCAN_DIRS[@]}" -g'*.rs' \
    --replace '$2 $1' \
    >>"$HTTP_OUT" || true

  # Handle chained methods like: get(...).post(...).delete(...)
  # We capture the chained `.method(...)` segments.
  rg -n --no-heading --pcre2 \
    '\.route\(\s*"([^"]+)"\s*,[^;]*\.\s*(get|post|put|delete|patch|head|options)\b' \
    "${CORE_SCAN_DIRS[@]}" -g'*.rs' \
    --replace '$2 $1' \
    >>"$HTTP_OUT" || true

  # Prefix hints: nest("/api", router)
  {
    echo
    echo "# prefix hints (nest/route_prefix style, best-effort)"
    rg -n --no-heading --pcre2 \
      '\.(nest|route_layer|layer)\(\s*"([^"]+)"' \
      "${CORE_SCAN_DIRS[@]}" -g'*.rs' \
      --replace '$1 $2' || true
  } >>"$HTTP_OUT"

  # Normalize duplicates while keeping file:line in output.
  # Format: file:line:METHOD<TAB>PATH
  if [ -s "$HTTP_OUT" ]; then
    awk 'NF {print $0}' "$HTTP_OUT" | sort -u >"$HTTP_OUT.tmp" && mv "$HTTP_OUT.tmp" "$HTTP_OUT"
  fi
fi

echo "==> extracting OpenAPI paths (JSON only, if present)..."
if have_rg; then
  # Find likely openapi specs; parse JSON specs for /paths.
  mapfile -t OPENAPI_JSON < <(rg --files -g'openapi*.json' -g'swagger*.json' -g'*openapi*.json' -g'*swagger*.json' . 2>/dev/null || true)
  if [ "${#OPENAPI_JSON[@]}" -gt 0 ]; then
    mapfile -t OPENAPI_JSON < <(printf "%s\n" "${OPENAPI_JSON[@]}" | sed -E 's|^\./||' | sort -u)
  fi
  if [ "${#OPENAPI_JSON[@]}" -gt 0 ]; then
    for f in "${OPENAPI_JSON[@]}"; do
      echo "# file: $f" >>"$OPENAPI_OUT"
      python3 - "$f" >>"$OPENAPI_OUT" <<'PY'
import json, sys
path = sys.argv[1]
with open(path, "r", encoding="utf-8") as fp:
    spec = json.load(fp)
paths = spec.get("paths") or {}
for p, methods in sorted(paths.items()):
    if not isinstance(methods, dict):
        continue
    for m in sorted(methods.keys()):
        if m.lower() in ("get","post","put","patch","delete","head","options","trace"):
            print(f"{m.upper()}\t{p}")
PY
      echo >>"$OPENAPI_OUT"
    done
  fi
fi

echo "==> extracting gRPC services/methods from .proto..."
if have_rg; then
  mapfile -t PROTOS < <(rg --files -g'*.proto' "${CORE_SCAN_DIRS[@]}" proto . 2>/dev/null || true)
  if [ "${#PROTOS[@]}" -gt 0 ]; then
    mapfile -t PROTOS < <(printf "%s\n" "${PROTOS[@]}" | sed -E 's|^\./||' | sort -u)
  fi
  if [ "${#PROTOS[@]}" -gt 0 ]; then
    for p in "${PROTOS[@]}"; do
      echo "# file: $p" >>"$GRPC_OUT"
      rg -n --no-heading '^\s*package\s+|^\s*service\s+|^\s*rpc\s+' "$p" \
        | sed -E 's/\r$//' \
        >>"$GRPC_OUT" || true
      echo >>"$GRPC_OUT"
    done
  fi
fi

echo "==> extracting UI routes (React Router heuristics)..."
if [ "${#PORTAL_SCAN_DIRS[@]}" -gt 0 ] && have_rg; then
  # React Router v7 file-based routing (flatRoutes from @react-router/fs-routes).
  # Example filename: dashboard.settings.email-templates.$type.tsx => /dashboard/settings/email-templates/:type
  for d in "${PORTAL_SCAN_DIRS[@]}"; do
    routes_dir="$d/app/routes"
    if [ -d "$routes_dir" ]; then
      find "$routes_dir" -maxdepth 1 -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.jsx' \) -print0 \
        | while IFS= read -r -d '' f; do
            base="$(basename "$f")"
            base="${base%.*}" # strip extension

            if [ "$base" = "_index" ]; then
              echo "/" >>"$UI_OUT"
              continue
            fi

            IFS='.' read -r -a parts <<<"$base"
            if [ "${#parts[@]}" -gt 0 ] && [ "${parts[-1]}" = "_index" ]; then
              unset 'parts[-1]'
            fi

            path=""
            for seg in "${parts[@]:-}"; do
              [ "$seg" = "_index" ] && continue
              if [[ "$seg" == \$* ]]; then
                seg=":${seg#\$}"
              fi
              path="$path/$seg"
            done

            [ -n "$path" ] && echo "$path" >>"$UI_OUT"
          done
    fi
  done

  # Common patterns:
  # - route objects: { path: "/x", element: ... }
  # - JSX routes: <Route path="/x" ... />
  rg --no-heading --no-filename --pcre2 \
    '\bpath\b\s*:\s*"([^"]+)"' \
    "${PORTAL_SCAN_DIRS[@]}" -g'*.{ts,tsx,js,jsx}' \
    --glob='!**/tests/**' --glob='!**/__tests__/**' --glob='!**/*.test.*' \
    --replace '$1' \
    >>"$UI_OUT" || true

  rg --no-heading --no-filename --pcre2 \
    '\<Route[^>]*\bpath\s*=\s*"([^"]+)"' \
    "${PORTAL_SCAN_DIRS[@]}" -g'*.{ts,tsx,js,jsx}' \
    --glob='!**/tests/**' --glob='!**/__tests__/**' --glob='!**/*.test.*' \
    --replace '$1' \
    >>"$UI_OUT" || true

  if [ -s "$UI_OUT" ]; then
    # Remove any accidental "file: ..." prefixes (shouldn't happen, but keep output clean).
    rg -v '^[^/]' "$UI_OUT" >"$UI_OUT.tmp" && mv "$UI_OUT.tmp" "$UI_OUT"
    sort -u "$UI_OUT" >"$UI_OUT.tmp" && mv "$UI_OUT.tmp" "$UI_OUT"
  fi
fi

echo "==> done."
echo
echo "outputs:"
echo "  - $HTTP_OUT"
echo "  - $OPENAPI_OUT"
echo "  - $GRPC_OUT"
echo "  - $UI_OUT"

echo
echo "==> initializing docs/security/_surface templates (if missing)..."
ASVS_PROFILE="$OUT_DIR/asvs_profile.md"
INPUT_INV="$OUT_DIR/input_inventory.md"

today="$(date +%Y-%m-%d)"

if [ ! -f "$ASVS_PROFILE" ] && [ -f "$REF_DIR/asvs-5.0-profile-template.md" ]; then
  # Best-effort: extract the fenced markdown block and fill dates.
  awk '
    /```markdown/ {inside=1; next}
    inside && /```/ {exit}
    inside {print}
  ' "$REF_DIR/asvs-5.0-profile-template.md" \
    | sed -E "s/\\{YYYY-MM-DD\\}/$today/g" \
    >"$ASVS_PROFILE" || true
fi

if [ ! -f "$INPUT_INV" ] && [ -f "$REF_DIR/input-inventory-template.md" ]; then
  cp "$REF_DIR/input-inventory-template.md" "$INPUT_INV" || true
fi

echo "  - $ASVS_PROFILE"
echo "  - $INPUT_INV"
