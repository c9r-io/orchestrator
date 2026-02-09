# Business Logic Security - Race Condition Tests (Generic)

**Module**: Business Logic Security  
**Scope**: Concurrent operations and TOCTOU (Time-of-Check to Time-of-Use)  
**Scenarios**: 4  
**Risk**: Critical  
**OWASP ASVS 5.0**: V2 Validation and Business Logic

---

## Background

Race conditions commonly occur in "check then act" flows:
- One-time tokens (check valid -> consume)
- Quotas/balances (check sufficient -> deduct)
- Uniqueness constraints (check missing -> create)
- State transitions (check state -> mutate)

---

## Scenario 1: Concurrent Use Of A One-Time Token (If Applicable)

### Preconditions
- A one-time token exists

### Attack Objective
Verify the token cannot be successfully consumed more than once within a concurrency window.

### Attack Steps
1. Prepare 20-50 concurrent requests using the same token
2. Send them at the same time
3. Count successes

### Expected Secure Behavior
- Success count <= 1
- Token consumption is atomic (transaction/lock/optimistic concurrency)

### Verification
```bash
BASE="${API_BASE_URL:-http://localhost:8080}"
TOKEN_ONCE="{one_time_token}"

seq 1 50 | xargs -P50 -I{} sh -c \
  "curl -s -o /dev/null -w '%{http_code}\n' -X POST '$BASE/api/v1/token/consume' -H 'Content-Type: application/json' -d '{\"token\":\"$TOKEN_ONCE\"}'"
```

---

## Scenario 2: Duplicate Creation Race (Uniqueness Constraints)

### Preconditions
- A resource is created by a unique key (slug/email/name)

### Attack Objective
Verify concurrent creates do not produce duplicates or inconsistent state.

### Attack Steps
1. Send 20 concurrent creates for the same unique key
2. Check success count and DB record count

### Expected Secure Behavior
- Only 1 succeeds; others return 409/400
- DB enforces unique constraints and app handles idempotency

---

## Scenario 3: Quota/Balance Race (If Applicable)

### Preconditions
- There is a quota or balance deduction logic

### Attack Objective
Verify concurrent deductions do not over-deduct or go negative.

### Attack Steps
1. Set a small balance/quota
2. Trigger concurrent deductions

### Expected Secure Behavior
- Balance never becomes negative
- Over-quota requests fail

---

## Scenario 4: State Machine Race (If Applicable)

### Preconditions
- State transitions exist (for example pending -> active -> disabled)

### Attack Objective
Verify concurrent transitions do not result in illegal states.

### Attack Steps
1. Trigger different transitions concurrently (enable/disable)
2. Check final state and audit records

### Expected Secure Behavior
- Final state respects defined transitions
- Illegal transitions are rejected

