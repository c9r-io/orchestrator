# Security Doc Style Guide

This guide keeps `docs/security/**` consistent across projects so developers and agents can execute and reuse it.

## Structure

Each document should include:
- Document header (module, scope, scenarios, risk, ASVS)
- Background (optional, but keep it short)
- Scenario 1..N (numbered scenarios)

## Placeholders

Use `{placeholder}` for dynamic values, for example:
- `{id}`, `{resource_id}`, `{token}`, `{user_a_token}`, `{admin_token}`

Do not put real secrets into documentation.

## Commands

Prefer commands a tester can run directly:
- HTTP: `curl`
- gRPC: `grpcurl` (if supported)
- Logs: `docker logs` / `kubectl logs` + `rg`
- Dependency audit: `cargo audit` / `npm audit` / `trivy`

## Risk Levels

- Critical: authorization bypass / large-scale data leakage / system takeover
- High: partial privilege issues / data leakage / major availability risk
- Medium: requires specific conditions or impact is limited
- Low: theoretical or low-impact risk

## ASVS Tagging (ASVS 5.0)

Use a header field like:
- `**OWASP ASVS 5.0**: V{chapter} ...`

At minimum, tag at the chapter level (for example `V6 Authentication, V7 Session Management`).
If you need requirement ids, use ASVS 5.0 text as the source of truth (do not invent ids).
Prefer `v5.0.0-<chapter>.<section>.<requirement>` format to avoid ambiguity.

