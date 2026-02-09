# Input Validation - SSRF Tests (Generic)

**Module**: Input Validation  
**Scope**: Server-side request forgery (URL fetch/webhooks/callbacks)  
**Scenarios**: 4  
**Risk**: Critical  
**OWASP ASVS 5.0**: V2 Validation and Business Logic

---

## Background

Applicable only if the project has features where the server initiates outbound HTTP/gRPC requests, such as:
- webhook/callback URLs
- URL preview, fetching, importing
- proxy download, image transcoding

Common targets:
- Cloud metadata: `169.254.169.254`
- Internal services: `127.0.0.1`, `10.0.0.0/8`, `192.168.0.0/16`
- DNS rebinding / redirect chains

---

## Scenario 1: Block Internal And Local Addresses

### Preconditions
- A feature exists that accepts a URL (for example `POST /api/v1/webhooks`)

### Attack Objective
Verify the server rejects requests to internal/local/link-local addresses.

### Attack Steps
1. Submit `http://127.0.0.1:...`
2. Submit `http://localhost:...`
3. Submit `http://169.254.169.254/latest/meta-data/`

### Expected Secure Behavior
- The request is rejected (400/403) and no outbound request is made
- Logs record the reason for blocking (without leaking sensitive content)

---

## Scenario 2: Redirect Chain Bypass

### Preconditions
- The SSRF target supports redirects (or you can host a redirecting service)

### Attack Objective
Verify following redirects still enforces the same address validation.

### Attack Steps
1. Submit a public URL that 302 redirects to an internal/metadata address

### Expected Secure Behavior
- The redirect destination is still blocked

---

## Scenario 3: DNS Rebinding (If Applicable)

### Preconditions
- A controllable domain and mutable DNS

### Attack Objective
Verify validation is based on the final resolved IP and has rebinding protections.

### Attack Steps
1. First resolve to a public IP to pass validation
2. Switch resolution to an internal IP before/during the connection

### Expected Secure Behavior
- Final IP is validated at connect time
- Internal addresses are blocked

---

## Scenario 4: Protocol And Port Restrictions

### Preconditions
- URLs can specify protocols and/or ports

### Attack Objective
Verify only necessary protocols (http/https) are allowed and ports are restricted.

### Attack Steps
1. Submit `file://`, `gopher://`, `ftp://` (if the implementation parses them)
2. Submit unexpected ports (for example `:22`, `:2375`, `:3306`)

### Expected Secure Behavior
- Disallowed protocols/ports are rejected

