# OWASP ASVS 5.0 Overview (Working Notes)

Goal: help `security-test-doc-gen` use ASVS 5.0 as the baseline control set when updating `docs/security/**`, and be explicit (in feature-only mode) about which chapters/requirements are triggered.

## ASVS Levels

- L1: baseline applications (minimum security baseline)
- L2: most applications with sensitive data, authenticated sessions, or real business assets (recommended default)
- L3: high-value targets / strong adversary environments (finance, identity, critical infrastructure, etc.)

## Requirement IDs (Format)

ASVS 5.0 requirement ids are numeric in the standard:
- `<chapter>.<section>.<requirement>` (for example `1.11.3`)
- In docs, prefer `v<version>-<chapter>.<section>.<requirement>` (for example `v5.0.0-1.2.5`) to reduce ambiguity across ASVS versions

Some materials/tools use a `V` prefix (for example `V6.6.1`) which refers to the same numbering system (chapter 6).

If you reference specific requirement ids, the ASVS 5.0 text is the source of truth.

## Chapter Map (High-level)

ASVS 5.0 chapters are organized as V1..Vn. Not every project needs to cover all chapters, but you must describe "which chapters were selected and why" in `docs/security/_surface/asvs_profile.md`.

Common chapters (roughly aligned with this repo's `docs/security` structure):
- V1 Encoding and Sanitization
- V2 Validation and Business Logic
- V3 Web Frontend Security
- V4 API and Web Service
- V5 File Handling
- V6 Authentication
- V7 Session Management
- V8 Authorization
- V9 Self Contained Tokens
- V10 OAuth & OIDC
- V11 Cryptography
- V12 Secure Communication
- V13 Configuration
- V14 Data Protection
- V15 Secure Coding and Architecture
- V16 Security Logging and Error Handling
- V17 WebRTC (if applicable)

Note: chapter names and sub-requirements must follow ASVS 5.0 text. This file is a working note for the skill; it does not replace the standard.

