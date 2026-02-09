# Infrastructure Security - TLS/Security Headers/CORS Baseline (Generic)

**Module**: Infrastructure Security  
**Scope**: TLS configuration, HTTP security headers, CORS policy  
**Scenarios**: 5  
**Risk**: Medium  
**OWASP ASVS 5.0**: V12 Secure Communication, V13 Configuration

---

## Background

Infrastructure misconfiguration is common and can have broad impact:
- Weak TLS versions/ciphers
- Missing security headers enabling clickjacking, MIME sniffing, etc.
- CORS misconfiguration enabling cross-origin reads of sensitive responses

---

## Scenario 1: TLS Version And Certificates (If Applicable)

### Preconditions
- The service is exposed via HTTPS (local or test env)

### Attack Objective
Verify minimum TLS version and certificate configuration.

### Attack Steps
1. Check whether TLS 1.0/1.1 is allowed
2. Check certificate chain validity and hostname matching

### Expected Secure Behavior
- TLS 1.0/1.1 disabled (TLS 1.2 minimum)
- Valid chain and correct hostname

---

## Scenario 2: Critical Security Headers

### Preconditions
- Able to inspect HTTP response headers

### Attack Objective
Verify a baseline set of security headers exists.

### Attack Steps
1. Request the Web UI or API root path
2. Check headers: `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`

### Expected Secure Behavior
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY` or CSP `frame-ancestors`
- Reasonable `Referrer-Policy` (for example `no-referrer` or `strict-origin-when-cross-origin`)

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
curl -I "$BASE/" | rg -i "x-content-type-options|x-frame-options|referrer-policy|content-security-policy|strict-transport-security" || true
```

---

## Scenario 3: HSTS (HTTPS Environments)

### Preconditions
- HTTPS is used

### Attack Objective
Verify `Strict-Transport-Security` configuration.

### Expected Secure Behavior
- HSTS enabled (recommended for production) with a reasonable max-age

---

## Scenario 4: CORS Allowlist And Credentials

### Preconditions
- Browser calls the API (or the service supports cross-origin requests)

### Attack Objective
Verify CORS does not allow arbitrary origins to read sensitive responses.

### Attack Steps
1. Check `Access-Control-Allow-Origin`
2. If `Access-Control-Allow-Credentials: true`, confirm `Allow-Origin` is not `*`

### Expected Secure Behavior
- Origin allowlist is enforced
- Credentials and origin configuration are consistent (no insecure combinations)

---

## Scenario 5: HTTP Methods And OPTIONS/TRACE

### Preconditions
- Service is reachable

### Attack Objective
Verify dangerous methods are disabled or controlled.

### Attack Steps
1. Probe `TRACE` and `TRACK`
2. Check whether OPTIONS leaks too much information (per project policy)

### Expected Secure Behavior
- TRACE/TRACK not supported

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
curl -i -X TRACE "$BASE/"
```

