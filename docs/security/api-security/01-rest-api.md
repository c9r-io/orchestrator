# API Security - REST API Tests (Generic)

**Module**: API Security  
**Scope**: REST endpoint protection, error handling, data exfiltration baseline  
**Scenarios**: 5  
**Risk**: High  
**OWASP ASVS 5.0**: V4 API and Web Service, V8 Authorization

---

## Background

These tests validate whether a "default deny" posture holds for a REST API. Common risks:
- Missing authentication/authorization leading to data leakage
- Loose token parsing leading to bypass
- Leftover deprecated/debug endpoints
- Pagination/export enabling bulk extraction
- Error messages leaking internal implementation details

---

## Scenario 1: Unauthenticated Access To Protected Endpoints

### Preconditions
- Collect the list of endpoints that should require authentication (router/OpenAPI/gateway config)

### Attack Objective
Verify every protected endpoint returns 401/403 without leaking data.

### Attack Steps
1. Enumerate endpoints: `METHOD PATH`
2. Call each endpoint without `Authorization`
3. Record any endpoints returning 2xx/3xx or sensitive content

### Expected Secure Behavior
- Non-public endpoints return 401 (unauthenticated) or 403 (authenticated but unauthorized)
- Error responses do not include stack traces, SQL, or internal service names

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"

# Example: replace with your project's endpoint list
ENDPOINTS=(
  "GET /api/v1/items"
  "POST /api/v1/items"
)

for e in "${ENDPOINTS[@]}"; do
  method=$(echo "$e" | awk '{print $1}')
  path=$(echo "$e" | awk '{print $2}')
  echo "Testing (no auth): $method $path"
  curl -s -o /dev/null -w "%{http_code}\n" -X "$method" "$BASE$path"
done
```

---

## Scenario 2: Token Validation Bypass And Location Variants

### Preconditions
- A valid token (`API_TOKEN`) and invalid samples (expired, tampered, malformed)

### Attack Objective
Verify token validation is strict and only accepts tokens from the standard location.

### Attack Steps
1. Use empty/malformed/expired/tampered tokens against protected endpoints
2. Attempt to pass tokens via query/cookies/non-standard headers

### Expected Secure Behavior
- All invalid tokens return 401
- Tokens are read only from `Authorization: Bearer ...` (unless the project explicitly supports other locations)

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"

curl -i -H "Authorization: Bearer " "$BASE/api/v1/items"
curl -i -H "Authorization: Bearer not.a.jwt" "$BASE/api/v1/items"

# Query token should not be accepted unless explicitly supported
curl -i "$BASE/api/v1/items?access_token=$TOKEN"
```

---

## Scenario 3: Legacy/Internal/Debug Endpoint Discovery

### Preconditions
- None

### Attack Objective
Verify there are no exposed legacy versions, internal endpoints, or debug endpoints.

### Attack Steps
1. Probe common legacy paths: `/api/v0/*`, unversioned `/api/*`
2. Probe internal paths: `/internal/*`, `/admin/*`, `/debug/*`
3. Probe common debug endpoints: `/metrics`, `/actuator`, `/debug/pprof`

### Expected Secure Behavior
- Return 404/405 or require strong authentication (per project policy)
- Debug endpoints are disabled in production or strictly restricted

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
curl -i "$BASE/api/v0/health"
curl -i "$BASE/api/users"
curl -i "$BASE/internal/config"
curl -i "$BASE/debug/pprof"
curl -i "$BASE/metrics"
```

---

## Scenario 4: Bulk Data Extraction Via Pagination/Filtering

### Preconditions
- A valid token for protected endpoints

### Attack Objective
Verify pagination limits and export/bulk limits prevent sustained large-scale scraping.

### Attack Steps
1. Try `limit=1000000`, `page=0&limit=0`, and negative values on list endpoints
2. If export endpoints exist, verify they require additional privileges and have throttling
3. Send concurrent requests and observe throttling/response size limits

### Expected Secure Behavior
- `limit` is clamped to a max (for example 100)
- Export/bulk operations require extra privilege and frequency limits

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"
curl -i -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/items?limit=1000000"
curl -i -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/items?limit=0"
curl -i -H "Authorization: Bearer $TOKEN" "$BASE/api/v1/items?limit=-1"
```

---

## Scenario 5: Error Handling And Information Leakage

### Preconditions
- Able to trigger 400/401/403/404/500 (pick a few endpoints)

### Attack Objective
Verify errors do not leak internals (stack traces, SQL, dependency endpoints, secrets).

### Attack Steps
1. Send malformed JSON, overly long fields, wrong types
2. Access a non-existent resource id
3. Trigger server exceptions (for example historical panic paths)

### Expected Secure Behavior
- Error responses are structured and stable
- Responses do not include stack traces, SQL, DSNs, private keys, or tokens

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"
curl -i -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"bad_json":' \
  "$BASE/api/v1/items"
```

