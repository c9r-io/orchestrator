# Business Logic Security - Workflow Abuse And Replay Tests (Generic)

**Module**: Business Logic Security  
**Scope**: Workflow step skipping, one-time token replay, idempotency and rollback  
**Scenarios**: 4  
**Risk**: Critical  
**OWASP ASVS 5.0**: V2 Validation and Business Logic

---

## Background

Common attacks against business workflows:
- Skipping prerequisite steps and directly calling later APIs
- Reusing one-time tokens (reset/invite/redeem)
- Missing idempotency causing double charges or duplicate creation

---

## Scenario 1: Step Skipping (Bypass Prerequisite Validation)

### Preconditions
- A multi-step workflow exists (for example create -> confirm -> execute)

### Attack Objective
Verify later steps cannot be called directly, or calling them forces the required validation.

### Attack Steps
1. Without performing step 1, call the API for step 2/3 directly
2. Forge workflow state fields (if any are client-controlled)

### Expected Secure Behavior
- Return 400/409/403
- Server enforces the real state machine and does not trust client-reported state

---

## Scenario 2: One-Time Token Replay (If Applicable)

### Preconditions
- A one-time token exists (invite/reset/redeem/confirmation link)

### Attack Objective
Verify tokens are single-use and become invalid immediately after use.

### Attack Steps
1. Obtain a token
2. Use it successfully once
3. Reuse the same token

### Expected Secure Behavior
- Second and subsequent attempts always fail
- Token consumption/update is atomic

---

## Scenario 3: Idempotency And Duplicate Submissions

### Preconditions
- Operations exist such as create/pay/redeem/send

### Attack Objective
Verify repeated submissions do not cause duplicate side effects.

### Attack Steps
1. Send the same request 10 times (same payload)
2. Send concurrently (see race-condition doc)

### Expected Secure Behavior
- There is an idempotency key and/or server-side deduplication
- Side effects occur at most once

---

## Scenario 4: Rollback And Intermediate Failure States

### Preconditions
- You can trigger mid-flow failures (downstream timeouts, validation failures)

### Attack Objective
Verify failures do not leave exploitable intermediate state (half-completed state).

### Attack Steps
1. Trigger a failure during the workflow
2. Check whether dirty data or reusable tokens are left behind

### Expected Secure Behavior
- Transactions/compensation ensure consistency
- Intermediate state cannot be exploited externally

