# Security Tests

This directory contains reproducible, verifiable security test documents (primarily black-box/gray-box style) for running security regression checks during development and before releases.

Goals:
- Cover common security risks (authentication, authorization, input validation, API security, session management, data security, infrastructure, logging, business logic, file handling).
- Lower the bar to execute tests by providing runnable steps/commands so developers and agents can reproduce results from the same docs.
- Stay generic: avoid domain-specific nouns; select only the docs/scenarios that apply to the project.

Conventions:
- Write everything in English. Keep technical details (API paths, headers, SQL, metric names) as-is.
- Mark dynamic values with `{placeholder}`.
- Keep each document to at most 5 numbered scenarios. Split into multiple documents if needed.
- ASVS tagging uses **OWASP ASVS 5.0** as the baseline. Default target level is L2 unless the project explicitly targets L1 or requires L3 for high-value systems.

## Environment Variables (Recommended)

```bash
API_BASE_URL="http://localhost:8080"
PORTAL_BASE_URL="http://localhost:3000"   # If there is a Web UI
API_TOKEN="{bearer_token}"                # If authentication is required
```

## Index

### Authentication
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/authentication/01-authentication.md` | Login/token/IdP integration regression checks | 5 | High |

### Authorization
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/authorization/01-access-control.md` | Access control, IDOR, privilege boundaries | 5 | Critical |

### Input Validation
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/input-validation/01-injection.md` | SQL/NoSQL/command injection | 5 | Critical |
| `docs/security/input-validation/02-xss.md` | XSS (stored/reflected/DOM) | 4 | High |
| `docs/security/input-validation/03-csrf.md` | CSRF (browser context) | 4 | High |
| `docs/security/input-validation/04-ssrf.md` | SSRF (callbacks/metadata/internal probing) | 4 | Critical |

### API Security
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/api-security/01-rest-api.md` | REST endpoint protection and data exfiltration | 5 | High |
| `docs/security/api-security/02-rate-limiting.md` | Rate limiting, brute force, enumeration, DoS baseline | 4 | High |

### Session Management
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/session-management/01-session-security.md` | Session/cookie regression checks | 4 | High |

### Data Security
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/data-security/01-sensitive-data.md` | Sensitive data exposure and masking | 5 | Critical |

### Infrastructure Security
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/infrastructure/01-tls-headers-cors.md` | TLS/security headers/CORS baseline | 5 | Medium |
| `docs/security/infrastructure/02-dependency-supply-chain.md` | Dependency audit, image scanning, supply chain | 4 | High |

### Logging And Monitoring Security
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/logging-monitoring/01-log-security.md` | Log injection, audit coverage, sensitive data in logs | 5 | High |

### Business Logic Security
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/business-logic/01-workflow-abuse.md` | Workflow abuse/replay/idempotency | 4 | Critical |
| `docs/security/business-logic/02-race-conditions.md` | Race conditions/TOCTOU | 4 | Critical |

### File Security (If Applicable)
| Doc | Description | Scenarios | Risk |
|------|------|--------|----------|
| `docs/security/file-security/01-file-upload.md` | Upload/download security | 4 | High |

## Execution Guidance (Strict)

1. Before every release, run at least: authentication, authorization, REST API, injection, sensitive data exposure, dependency audit.
2. If the project has a Web UI (cookie-based sessions): also run CSRF/XSS/session docs.
3. If the project has webhooks/URL fetch/callbacks: also run SSRF docs.
4. If the project has concurrency-sensitive operations (one-time tokens, redemption, payments, invites, quotas): also run business logic and race-condition docs.
