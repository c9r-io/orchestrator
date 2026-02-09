# ASVS 5.0 Profile Template

Use this template to generate `docs/security/_surface/asvs_profile.md`.

```markdown
# ASVS 5.0 Security Profile

**Target Level**: L1 / L2 / L3
**Scope**: feature-only / system-baseline
**Created**: {YYYY-MM-DD}
**Updated**: {YYYY-MM-DD}

## System Summary
- Runtime: {language/framework}
- Interfaces: HTTP / gRPC / UI
- AuthN: {jwt/session/oidc/...}
- AuthZ: {rbac/abac/tenant-scope/...}
- Data: {PII/secrets/payments/...}

## Applicable ASVS Chapters
- {V1} Encoding and Sanitization: {Yes/No + reason}
- {V2} Validation and Business Logic: {Yes/No + reason}
- {V3} Web Frontend Security: {Yes/No + reason}
- {V4} API and Web Service: {Yes/No + reason}
- {V5} File Handling: {Yes/No + reason}
- {V6} Authentication: {Yes/No + reason}
- {V7} Session Management: {Yes/No + reason}
- {V8} Authorization: {Yes/No + reason}
- {V9} Self Contained Tokens: {Yes/No + reason}
- {V10} OAuth & OIDC: {Yes/No + reason}
- {V11} Cryptography: {Yes/No + reason}
- {V12} Secure Communication: {Yes/No + reason}
- {V13} Configuration: {Yes/No + reason}
- {V14} Data Protection: {Yes/No + reason}
- {V15} Secure Coding and Architecture: {Yes/No + reason}
- {V16} Security Logging and Error Handling: {Yes/No + reason}
- {V17} WebRTC: {Yes/No + reason}

## Security Assumptions
- {e.g. TLS terminated at ingress}
- {e.g. no file upload in current system}

## Evidence Sources
- Routes/Surface: `docs/security/_surface/*`
- Key code paths: {file paths}
```
