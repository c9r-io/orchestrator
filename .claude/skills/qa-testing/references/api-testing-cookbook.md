# QA API Testing Cookbook

Quick-reference recipes for common QA API/gRPC/webhook tasks.

## 1. gRPC Testing Through Docker Network

Key points:
- Reflection may be disabled; provide `-import-path` and `-proto`.
- Host ports can be blocked; run grpcurl from Docker network.
- Some endpoints require mTLS and API keys.

Example:

```bash
.claude/skills/tools/grpcurl-docker.sh \
  -import-path /proto -proto my.proto \
  -d '{"hello":"world"}' \
  my-grpc:50051 my.pkg.Service/MyMethod
```

## 2. Signed Webhook Testing

Key points:
- Build signature from the exact raw body.
- Ensure header format matches server expectation.

Example (`sha256=<hex>` style):

```bash
BODY='{"type":"EVENT","id":"123"}'
SECRET='dev-webhook-secret'
SIGNATURE=$(echo -n "$BODY" | openssl dgst -sha256 -hmac "$SECRET" | awk '{print $2}')

curl -s -X POST http://localhost:8080/webhooks/events \
  -H "Content-Type: application/json" \
  -H "x-signature: sha256=$SIGNATURE" \
  -d "$BODY"
```

## 3. Common Failure Patterns

| Error pattern | Likely cause | First fix |
|---|---|---|
| Reflection error | Missing proto args | Add `-import-path` and `-proto` |
| Auth missing | Missing API key or bearer token | Inject required headers |
| TLS handshake failure | Missing/wrong client cert | Provide correct `-cacert/-cert/-key` |
| Timeout on localhost | Host-to-container routing issue | Run from compose network |
