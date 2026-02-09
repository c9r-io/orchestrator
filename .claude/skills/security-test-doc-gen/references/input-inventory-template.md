# Input Inventory Template (ASVS 5.0 aligned)

Use this template as the skeleton for `docs/security/_surface/input_inventory.md`. It supports multiple chapters (especially V1/V2/V4/V5) and is not only for injection tests.

## HTTP Endpoints

| Method | Path | AuthN | AuthZ/Scope | Inputs (path/query/header/body/cookie) | Constraints (type/range/len/format) | High-Risk Sinks |
|--------|------|-------|------------|----------------------------------------|-------------------------------------|-----------------|
| GET | /api/v1/items | required | tenant-scope | `q` (query), `limit` (query) | `limit<=100` | DB query, logging |

## gRPC Methods

| Service | RPC | AuthN | AuthZ/Scope | Request Fields | Constraints | High-Risk Sinks |
|---------|-----|------|------------|----------------|------------|-----------------|
| TokenExchange | ExchangeToken | required | service-scope | `identity_token`, `tenant_id` | uuid format | token signing, DB |

## UI Routes & Forms (if any)

| Route | Form | Fields | Constraints | Notes |
|------|------|--------|------------|------|
| /login | Login | email, password | email format, pw length | rate limiting |

