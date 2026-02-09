# OWASP ASVS 5.0 - Input Validation Checklist (High Bar)

Purpose: when a feature introduces or modifies external inputs, use ASVS 5.0 input-related chapters as a hard gate and turn "input validation" into reproducible tests, not just a few payload probes.

Notes:
- ASVS 5.0 chapters/requirements must follow the official text. This checklist is an "executable high bar" interpretation.
- This checklist primarily covers V1 Encoding and Sanitization and V2 Validation and Business Logic, and couples with V4 API and Web Service for API scenarios.

## Step 0: Input Inventory (Required)

First generate/update:
- `docs/security/_surface/input_inventory.md`

For each endpoint/method, list at minimum:
- Input locations: path/query/header/body/cookie/multipart
- Field constraints: required/type/range/length/format/enum
- High-risk sinks: DB queries, template rendering, file paths, outbound URLs, deserialization, logging

## Step 1: Universal Field Tests (Default)

For each field, cover at least the following (trim by type, but document why you trimmed):
- Missing: missing required field
- Empty: `""`, `null`, whitespace-only
- Type mismatch: string<->number, object<->string, array<->scalar
- Boundaries: min/max length, min/max value, 0/negative, very large integers
- Oversized: per-field (1KB/64KB/1MB), verify size limits/timeouts
- Unknown fields: over-posting / mass assignment
- Duplicates: `?a=1&a=2`, duplicate headers, duplicate JSON keys (parser differences)
- Encoding/normalization: URL encoding/double encoding/Unicode normalization (NFC/NFD)
- Control chars: `\r\n\t`, zero-width characters, bidi control chars (especially if logged or reflected to UI)
- Content-Type: missing/incorrect `Content-Type`
- Error responses: stable 4xx without leaking internals (stack traces/SQL/paths/secrets)

## Step 2: High-risk Input Types

### A) URL / webhook / callback (SSRF)
- Protocol allowlist (http/https)
- Host allowlist or strict blocks for internal/metadata ranges
- Redirect chain + final destination validation
- DNS rebinding protection (validate final IP at connect time)
- Port policy, timeouts, max response size

### B) File name/path/object key (Traversal)
- Reject `../`, absolute paths, encoded variants, backslash variants
- Server generates safe keys/names

### C) Rich text/HTML/Markdown (XSS)
- Output encoding + sanitizer
- SVG policy (download vs safe rendering)
- CSP baseline (if UI exists)

### D) Deserialization / object mapping
- Disable/limit polymorphic deserialization
- Strict schema (reject unknown fields)
- Depth/recursion/element-count limits

### E) Search/filter/sort
- Allowlist filterable/sortable fields
- Disallow raw expressions
- ReDoS protections (limit user regex or set timeouts/complexity budgets)

## "Pass" Definition (High Bar)

- Server has explicit schema/constraints (not only frontend validation)
- Default deny: invalid input is rejected or safely normalized and logged (per project policy)
- Invalid input does not produce side effects
- Responses and logs do not leak secrets/implementation details

