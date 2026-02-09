# Input Validation - CSRF Tests (Generic)

**Module**: Input Validation  
**Scope**: Browser-context request forgery (cookie sessions / automatic credential sending)  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V3 Web Frontend Security, V7 Session Management

---

## Background

Applicable only if the project uses cookie-based sessions (or browsers automatically attach credentials). If the API uses only `Authorization: Bearer` and does not accept cookie tokens, CSRF risk is usually much lower, but still verify:
- Whether cookies and bearer tokens are mixed
- Whether any GET request causes state changes
- Whether CORS misconfiguration allows cross-site readable responses

---

## Scenario 1: CSRF Protection On State-Changing Endpoints

### Preconditions
- Logged-in browser session (cookies present)
- State-changing endpoints exist (POST/PUT/PATCH/DELETE)

### Attack Objective
Verify cross-site requests cannot perform sensitive operations under a victim's session.

### Attack Steps
1. On an attacker site, construct an auto-submitting form or fetch request
2. Point it at a state-changing URL on the victim site
3. Observe whether the state change succeeds

### Expected Secure Behavior
- Require CSRF tokens and/or block via SameSite policy
- Cross-site requests fail with 403/400

---

## Scenario 2: SameSite/Cookie Attributes

### Preconditions
- Ability to inspect `Set-Cookie`

### Attack Objective
Confirm cookies are configured to minimize CSRF risk.

### Attack Steps
1. Check `SameSite`, `Secure`, `HttpOnly`

### Expected Secure Behavior
- `SameSite=Lax` or `Strict` (based on business compatibility)
- `Secure` (HTTPS) and `HttpOnly`

### Verification
```bash
BASE="${PORTAL_BASE_URL:-http://localhost:3000}"
curl -I "$BASE/" | rg -i "set-cookie" || true
```

---

## Scenario 3: GET Must Not Cause State Changes

### Preconditions
- GET routes exist

### Attack Objective
Verify GET does not create/update/delete or cause other side effects ("click-to-pwn").

### Attack Steps
1. Audit GET routes
2. For any suspicious route, call it and observe whether it mutates state

### Expected Secure Behavior
- GET is read-only
- State changes use POST/PUT/PATCH/DELETE

---

## Scenario 4: Cross-Origin Readability And Preflight (CORS Coupling)

### Preconditions
- Browser-accessible APIs exist

### Attack Objective
Verify CORS does not allow arbitrary origins to read sensitive responses with credentials.

### Attack Steps
1. Check `Access-Control-Allow-Origin` and `Access-Control-Allow-Credentials`
2. If `Allow-Credentials: true`, verify `Allow-Origin` is not `*` and is an allowlist

### Expected Secure Behavior
- Origin allowlist is enforced
- Arbitrary cross-site reading of sensitive responses is not allowed

