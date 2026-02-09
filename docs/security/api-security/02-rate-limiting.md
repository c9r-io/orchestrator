# API Security - Rate Limiting And DoS Protections (Generic)

**Module**: API Security  
**Scope**: Brute force, enumeration, concurrency and resource-consumption baseline  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V4 API and Web Service, V2 Validation and Business Logic

---

## Background

Even with correct authentication/authorization, missing rate limiting can enable:
- Brute force and password spraying
- Resource enumeration (id/slug/email)
- High concurrency leading to application-layer DoS

---

## Scenario 1: Login Brute Force (If Applicable)

### Preconditions
- A login/token-exchange endpoint exists (for example `/login`, `/auth/token`)

### Attack Objective
Verify failed-login throttling, exponential backoff, account lockout, or challenge (captcha/mfa) behavior.

### Attack Steps
1. Attempt 50 wrong passwords for the same account
2. Spray across multiple accounts with the same weak password
3. Observe status codes, latency, and whether challenges are triggered

### Expected Secure Behavior
- Rate limiting (429) and/or challenges (captcha/mfa) are triggered
- Audit/alerting records the attack pattern

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"

# Replace with the project's actual login endpoint
for i in $(seq 1 50); do
  curl -s -o /dev/null -w "%{http_code}\n" \
    -X POST "$BASE/login" \
    -H "Content-Type: application/json" \
    -d '{"username":"{user}","password":"wrong"}'
done
```

---

## Scenario 2: Rate Limiting On Resource Probing And Enumeration

### Preconditions
- An endpoint exists that looks up by id/slug (for example `GET /api/v1/items/{id}`)

### Attack Objective
Verify probing 404/403/401 responses is also rate limited to reduce enumerability.

### Attack Steps
1. Request 200 random ids (non-existent)
2. Observe whether 429 is triggered or latency increases

### Expected Secure Behavior
- Rate limiting exists and/or a unified response policy is used (to reduce enumerability)

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"

for i in $(seq 1 200); do
  curl -s -o /dev/null -w "%{http_code}\n" \
    -H "Authorization: Bearer $TOKEN" \
    "$BASE/api/v1/items/{random_id_$i}"
done
```

---

## Scenario 3: Large Bodies / Slow Requests (If Applicable)

### Preconditions
- A POST/PUT endpoint accepts JSON

### Attack Objective
Verify request body size limits, timeouts, and slow-request protections.

### Attack Steps
1. Send an overly large JSON body (for example 5-20MB)
2. Send chunked/slow uploads (if testable at gateway level)

### Expected Secure Behavior
- 413 Payload Too Large or 400
- Request timeouts are configurable (to mitigate slow DoS)

---

## Scenario 4: Concurrency Protection For Expensive Endpoints (If Applicable)

### Preconditions
- Expensive operations exist: export, reports, search, full-text, external calls

### Attack Objective
Verify expensive endpoints have per-user concurrency limits and global protections.

### Attack Steps
1. Send 50-200 concurrent requests to a single expensive endpoint
2. Observe error rate, queueing, timeouts, circuit breakers

### Expected Secure Behavior
- Rate limiting (429) and/or queueing is triggered
- Overall service remains available (other endpoints are not dragged down)

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"

# Requires GNU parallel or xargs -P
seq 1 100 | xargs -P50 -I{} sh -c \
  "curl -s -o /dev/null -w '%{http_code}\n' -H 'Authorization: Bearer $TOKEN' '$BASE/api/v1/search?q=test'"
```

