# Input Validation - XSS Tests (Generic)

**Module**: Input Validation  
**Scope**: Stored/reflected/DOM XSS (if a Web UI exists)  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V1 Encoding and Sanitization, V3 Web Frontend Security

---

## Background

Applicable only if the project has a Web UI or returns HTML/rich text. Common risk areas:
- User-controlled content rendered into HTML (comments, names, descriptions)
- Markdown/rich-text rendering without proper sanitization
- Missing or misconfigured CSP

---

## Scenario 1: Stored XSS (If Applicable)

### Preconditions
- A write-then-display field exists (for example name/description/comment)

### Attack Objective
Verify persisted content is properly escaped/sanitized on display.

### Attack Steps
1. Submit payload: `<img src=x onerror=alert(1)>`
2. Open the display page or list page
3. Observe whether scripts execute

### Expected Secure Behavior
- No script execution
- Payload is escaped or safely filtered

---

## Scenario 2: Reflected XSS (If Applicable)

### Preconditions
- A page reflects query parameters (search pages, error pages)

### Attack Objective
Verify reflected content is escaped.

### Attack Steps
1. Visit a URL with payload: `?q=<svg/onload=alert(1)>`
2. Observe whether scripts execute

### Expected Secure Behavior
- No script execution
- Reflected content is escaped

---

## Scenario 3: DOM XSS (If Applicable)

### Preconditions
- Frontend uses `innerHTML`, `dangerouslySetInnerHTML`, or similar APIs

### Attack Objective
Verify untrusted input is not inserted into the DOM as HTML.

### Attack Steps
1. Inject into a controllable field: `\"><img src=x onerror=alert(1)>`
2. Trigger frontend rendering logic (dialog, rich-text preview)

### Expected Secure Behavior
- No script execution
- Untrusted input is treated as plain text

---

## Scenario 4: CSP Baseline (If Applicable)

### Preconditions
- Web UI is reachable

### Attack Objective
Verify CSP exists and is reasonable (to reduce XSS impact).

### Attack Steps
1. Check the `Content-Security-Policy` response header
2. Attempt inline script injection (if injection is possible)

### Expected Secure Behavior
- CSP is present and does not allow `unsafe-inline` (as feasible for the project)
- Scripts are allowed only from trusted sources

### Verification
```bash
BASE="${PORTAL_BASE_URL:-http://localhost:3000}"
curl -I "$BASE/" | rg -i "content-security-policy|x-content-type-options|x-frame-options" || true
```

