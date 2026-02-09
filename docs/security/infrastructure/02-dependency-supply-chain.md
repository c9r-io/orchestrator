# Infrastructure Security - Dependency Audit And Supply Chain (Generic)

**Module**: Infrastructure Security  
**Scope**: Third-party dependencies, container image vulnerabilities, supply chain baseline  
**Scenarios**: 4  
**Risk**: High  
**OWASP ASVS 5.0**: V15 Secure Coding and Architecture, V13 Configuration

---

## Background

Supply chain risk sources:
- Known vulnerabilities (CVEs/advisories)
- Malicious packages and typosquatting
- Base image and OS package vulnerabilities

---

## Scenario 1: Rust Dependency Audit (If Applicable)

### Preconditions
- `Cargo.lock` exists

### Attack Objective
Find known vulnerabilities and deprecated Rust dependencies.

### Attack Steps
1. Run `cargo audit`
2. Run `cargo outdated` (optional)
3. Assess impact of HIGH/CRITICAL findings

### Expected Secure Behavior
- No unaddressed high-severity vulnerabilities, or there is explicit risk acceptance documentation
- Automated scanning in CI (recommended)

### Verification
```bash
cd core 2>/dev/null || true
cargo audit
cargo outdated || true
```

---

## Scenario 2: Node.js Dependency Audit (If Applicable)

### Preconditions
- `package-lock.json`/`pnpm-lock.yaml`/`yarn.lock` exists

### Attack Objective
Find JS dependency vulnerabilities and supply chain anomalies.

### Attack Steps
1. Run `npm audit --omit=dev` (or pnpm/yarn equivalent)
2. Check whether high-severity issues are upgrade-fixable

### Expected Secure Behavior
- No high-severity vulnerabilities in production dependencies
- A review path exists for introducing new dependencies

### Verification
```bash
cd portal 2>/dev/null || true
npm audit --omit=dev || true
```

---

## Scenario 3: Docker Image Scanning (If Applicable)

### Preconditions
- Images can be built

### Attack Objective
Find OS/package vulnerabilities and common configuration risks in images.

### Attack Steps
1. Scan images with Trivy or Docker Scout
2. Focus on HIGH/CRITICAL

### Expected Secure Behavior
- Vulnerability level is acceptable (or has explicit exceptions and an upgrade plan)

### Verification
```bash
# Example: replace with the actual image name
trivy image {image_name}:latest || true
```

---

## Scenario 4: Supply Chain Baseline Checks

### Preconditions
- Access to repo and CI configuration

### Attack Objective
Reduce typosquatting and malicious dependency risk.

### Attack Steps
1. Verify registries are official sources
2. Verify lock files exist and are committed
3. Review newly added dependencies (especially low-download packages)

### Expected Secure Behavior
- Lock files exist and are protected
- Dependency sources are explicit and arbitrary script execution is minimized

