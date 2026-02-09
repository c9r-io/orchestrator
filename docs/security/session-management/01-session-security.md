# Session Management - Session/Cookie Security Tests (Generic)

**Module**: Session Management  
**Scope**: Session generation, fixation, hijacking, expiration policies (if a Web UI exists)  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V7 Session Management

---

## Background

Applicable only if the project uses browser sessions (cookies) or a server-side session store. If the API is bearer-token-only with no cookies, session risks shift to token lifetime and revocation strategies.

---

## Scenario 1: Session ID And Cookie Attributes

### Preconditions
- Able to capture `Set-Cookie` (before and after login)

### Attack Objective
Verify session id randomness and cookie attributes.

### Attack Steps
1. Obtain 10 session ids
2. Check length and charset and whether it looks predictable
3. Check cookie flags: `HttpOnly`, `Secure`, `SameSite`

### Expected Secure Behavior
- Session ids are generated with a CSPRNG and have sufficient entropy
- `HttpOnly`, `Secure`, `SameSite` are configured appropriately

---

## Scenario 2: Session Fixation

### Preconditions
- You can send a custom cookie in requests

### Attack Objective
Verify session id rotates after login and the old session becomes invalid.

### Attack Steps
1. Obtain a pre-login session id
2. Login while presenting that session
3. Verify a new session id is issued after successful login
4. Use the old session id to access protected pages

### Expected Secure Behavior
- New session id after login
- Old session becomes invalid

---

## Scenario 3: Session Hijacking And Anomaly Detection (If Applicable)

### Preconditions
- A valid session exists

### Attack Objective
Verify cross-device or cross-IP session reuse is detectable/blockable (per project policy).

### Attack Steps
1. Reuse the same session cookie in another browser
2. Modify `User-Agent` and `X-Forwarded-For` (if a trusted proxy chain exists)
3. Observe whether re-authentication or alerting occurs

### Expected Secure Behavior
- At minimum, audit logs record anomalies
- High-value systems may require re-authentication

---

## Scenario 4: Expiration And Concurrency Controls (If Applicable)

### Preconditions
- Idle timeout and/or absolute timeout policy exists, and/or a concurrent session limit exists

### Attack Objective
Verify timeouts and concurrency limits are enforced.

### Attack Steps
1. Login and stay idle beyond the idle timeout
2. Login on multiple devices exceeding any configured limit

### Expected Secure Behavior
- After timeout, the user must re-authenticate
- Concurrent sessions have a limit or are manageable per policy

