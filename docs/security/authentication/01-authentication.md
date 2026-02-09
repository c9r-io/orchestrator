# Authentication Security - Login And Token Tests (Generic)

**Module**: Authentication  
**Scope**: Login, token issuance and validation, IdP integration (if applicable)  
**Scenarios**: 5  
**Risk**: High  
**OWASP ASVS 5.0**: V6 Authentication, V9 Self Contained Tokens, V10 OAuth & OIDC

---

## Background

Common failure modes in authentication systems:
- Login endpoints missing brute-force protection
- Weak token validation (alg/iss/aud/exp/nbf/sub)
- Missing token binding leading to long-lived lateral movement after theft
- Misconfigured IdP/OIDC integrations causing token confusion or callback hijacking

---

## Scenario 1: Public Endpoint Allowlist Audit

### Preconditions
- Collect all externally exposed HTTP routes (router/OpenAPI/gateway)

### Attack Objective
Verify only necessary endpoints are public and everything else requires authentication by default.

### Attack Steps
1. Call each endpoint without authentication
2. Mark endpoints that should be public (for example `/health`, `/.well-known/*`)
3. Analyze any unexpectedly public business endpoints

### Expected Secure Behavior
- The public endpoint set is small and explicit
- No business/data endpoints are accidentally public

---

## Scenario 2: Strict Token Validation (If Applicable)

### Preconditions
- A valid token (`API_TOKEN`) and invalid samples (expired, tampered)

### Attack Objective
Verify token validation includes signature verification and critical claims.

### Attack Steps
1. Use an expired token against a protected endpoint
2. Tamper with payload (modify role/exp) and try again
3. Use a token with different issuer or audience and try again

### Expected Secure Behavior
- All attempts return 401
- Error messages do not leak signature-validation details

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
curl -i -H "Authorization: Bearer {expired_or_tampered_token}" "$BASE/api/v1/items"
```

---

## Scenario 3: Token Scope And Least Privilege

### Preconditions
- At least two tokens with different privileges (regular user vs admin)

### Attack Objective
Verify high-privilege operations require a high-privilege token and cannot be escalated by request tampering.

### Attack Steps
1. Use a low-privilege token to access admin endpoints
2. Attempt role/scope injection via header/query/body

### Expected Secure Behavior
- 403 (authenticated but not authorized)
- Authorization decisions are based on server-side policy/claims; do not trust client-reported fields

---

## Scenario 4: Logout And Revocation (If Applicable)

### Preconditions
- The system supports any of: logout, token revoke, session revoke

### Attack Objective
Verify after logout/revocation, the token is no longer usable (or expires within a defined policy window).

### Attack Steps
1. Login and obtain a token
2. Call logout/revoke endpoint
3. Reuse the old token on a protected endpoint

### Expected Secure Behavior
- Old token is immediately invalid, or invalid within a clearly defined window
- Server records revocation events (audit)

---

## Scenario 5: IdP/OIDC Integration Security (If Applicable)

### Preconditions
- The system integrates with an external IdP/OIDC provider

### Attack Objective
Verify callback URL, state/nonce, issuer/audience validation are correct.

### Attack Steps
1. Attempt callback URL redirection (open redirect) and callback hijacking
2. Replay an old code or id_token
3. Attempt exchange with a token from the wrong issuer

### Expected Secure Behavior
- Strict state/nonce validation
- `redirect_uri` allowlist is fixed
- Strict issuer/audience validation

