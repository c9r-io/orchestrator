#!/usr/bin/env bash
set -euo pipefail

# Best-effort extraction of a project's externally visible UI surface:
# - UI routes (React Router heuristics + file-based routes)
# - Theme hooks hints (data-theme / localStorage key)
#
# Output is intended to be used as an input when tailoring docs/uiux/**.

ROOT="${ROOT:-$(pwd)}"
OUT_DIR="${OUT_DIR:-docs/uiux/_surface}"
PORTAL_DIRS="${PORTAL_DIRS:-}"

cd "$ROOT"
mkdir -p "$OUT_DIR"

have_rg() { command -v rg >/dev/null 2>&1; }
ts() { date +"%Y-%m-%d %H:%M:%S"; }

split_csv() {
  local s="${1:-}"
  if [ -z "$s" ]; then
    return 0
  fi
  echo "$s" | tr ',' '\n' | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' | rg -v '^$' || true
}

pick_dirs() {
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

echo "# uiux surface extract" >"$OUT_DIR/README.txt"
echo "generated_at=$(ts)" >>"$OUT_DIR/README.txt"
echo "root=$ROOT" >>"$OUT_DIR/README.txt"
echo >>"$OUT_DIR/README.txt"

UI_ROUTES_OUT="$OUT_DIR/ui_routes.txt"
THEME_HINTS_OUT="$OUT_DIR/theme_hints.txt"

: >"$UI_ROUTES_OUT"
: >"$THEME_HINTS_OUT"

PORTAL_SCAN_DIRS=()
while IFS= read -r d; do PORTAL_SCAN_DIRS+=("$d"); done < <(pick_dirs "$PORTAL_DIRS" portal auth9-portal frontend web ui apps)

echo "==> extracting UI routes (heuristics)..."
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
              echo "/" >>"$UI_ROUTES_OUT"
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

            [ -n "$path" ] && echo "$path" >>"$UI_ROUTES_OUT"
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
    >>"$UI_ROUTES_OUT" || true

  rg --no-heading --no-filename --pcre2 \
    '\<Route[^>]*\bpath\s*=\s*"([^"]+)"' \
    "${PORTAL_SCAN_DIRS[@]}" -g'*.{ts,tsx,js,jsx}' \
    --glob='!**/tests/**' --glob='!**/__tests__/**' --glob='!**/*.test.*' \
    --replace '$1' \
    >>"$UI_ROUTES_OUT" || true

  if [ -s "$UI_ROUTES_OUT" ]; then
    rg -v '^[^/]' "$UI_ROUTES_OUT" >"$UI_ROUTES_OUT.tmp" && mv "$UI_ROUTES_OUT.tmp" "$UI_ROUTES_OUT"
    sort -u "$UI_ROUTES_OUT" >"$UI_ROUTES_OUT.tmp" && mv "$UI_ROUTES_OUT.tmp" "$UI_ROUTES_OUT"
  fi
fi

echo "==> extracting theme hints..."
if [ "${#PORTAL_SCAN_DIRS[@]}" -gt 0 ] && have_rg; then
  {
    echo "# data-theme usage"
    rg -n --no-heading --pcre2 'data-theme|dataset\.theme|setAttribute\\(\\s*[\"\\x27]data-theme' "${PORTAL_SCAN_DIRS[@]}" -S -g'*.{ts,tsx,js,jsx,html,css}' || true
    echo
    echo "# localStorage theme key usage"
    rg -n --no-heading --pcre2 'localStorage\\.(getItem|setItem)\\(\\s*[\"\\x27]theme[\"\\x27]' "${PORTAL_SCAN_DIRS[@]}" -S -g'*.{ts,tsx,js,jsx}' || true
  } >>"$THEME_HINTS_OUT"
fi

echo "==> done."
echo
echo "outputs:"
echo "  - $UI_ROUTES_OUT"
echo "  - $THEME_HINTS_OUT"

