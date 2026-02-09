# File Security - Upload/Download Tests (Generic)

**Module**: File Security  
**Scope**: Upload validation, path traversal, content sniffing, size limits (if applicable)  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V5 File Handling

---

## Background

Applicable only if the project supports file upload/download. Common risks:
- Validating only by extension allows executable/script uploads
- Path traversal (`../`) overwrites files
- SVG/HTML treated as "images" triggers XSS
- Large files cause resource exhaustion

---

## Scenario 1: File Type And Content Validation

### Preconditions
- An upload endpoint exists (for example `POST /api/v1/uploads`)

### Attack Objective
Verify validation includes both MIME and magic bytes, and executable/script types are rejected.

### Attack Steps
1. Upload `test.php`, `test.jsp`, `test.html`
2. Upload a polyglot where extension is png but content is HTML
3. Upload `svg` and observe display/download behavior

### Expected Secure Behavior
- Dangerous types are rejected
- If SVG is allowed, it must be handled safely (download instead of inline rendering)

---

## Scenario 2: Path Traversal And Filename Handling

### Preconditions
- Upload allows custom filenames or the service echoes the original filename

### Attack Objective
Verify filenames cannot influence storage paths and `../` and special characters are blocked.

### Attack Steps
1. Set filename to `../../etc/passwd`
2. Set filename to include `\r\n` (header splitting)

### Expected Secure Behavior
- Server generates safe filenames and uses a fixed storage path
- Response headers are not injectable

---

## Scenario 3: Size Limits And Decompression Bombs

### Preconditions
- Upload is enabled

### Attack Objective
Verify request body size, per-file size, and decompressed-size limits.

### Attack Steps
1. Upload an oversized file (> limit)
2. Upload a zip bomb (if decompression is supported)

### Expected Secure Behavior
- Return 413 or 400
- Service does not OOM or block for a long time

---

## Scenario 4: Download Access Control (IDOR)

### Preconditions
- Uploaded files are downloadable via URL

### Attack Objective
Verify download links are not enumerable and access is permissioned.

### Attack Steps
1. User B uploads a file and gets `{file_id}`
2. User A downloads `{file_id}` directly

### Expected Secure Behavior
- 403/404
- Download links have TTL (optional)

