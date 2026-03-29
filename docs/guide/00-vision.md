# 00 - Vision

Agent Orchestrator is a **Harness Engineering control plane** for agent-first software delivery.

OpenAI recently used the term [Harness Engineering](https://openai.com/index/harness-engineering/) to describe an engineering style where humans focus less on hand-writing code and more on designing environments, specifying intent, and building feedback loops that let agents do reliable work. This project adopts that framing directly.

## What We Are Building

- A local-first control plane that abstracts Claude Code, Codex, OpenCode, Gemini CLI, and other shell-based tools behind one resource model
- A declarative harness where workflows, agents, triggers, secrets, policies, and observability live as versioned repository assets
- A long-running execution runtime that can keep plan -> implement -> test -> review -> fix loops alive for hours or days

## Why a Control Plane?

A serious engineering harness does more than invoke an agent once. It decides:

- which agent should run
- in which workspace
- on which workflow step
- with which guardrails and secrets
- under which retry, pause, or recovery policy
- and how results are turned into auditable state, logs, and downstream actions

That is the role of this project.

## Core Beliefs

- **Humans steer, agents execute**: people set goals, acceptance criteria, and constraints; the system coordinates execution.
- **The repository is the system of record**: manifests, docs, skills, QA artifacts, and policies should be versioned and discoverable in-repo.
- **Guardrails matter more than heroic prompting**: workflows, invariants, triggers, and recovery paths compound more than one-off prompts.
- **Any shell-based agent should be portable**: the control plane should outlive any single model or vendor.
- **Long-running software delivery loops must be observable**: durable state, logs, events, and traces are first-class requirements.

## What This Is Not

- Not a chat wrapper around one model
- Not just a workflow DSL
- Not a generic CI/CD replacement for every enterprise use case

The ambition is narrower and more opinionated: become the control plane for agent-native engineering systems.

## Product Direction

- Turn ad hoc agent usage into reusable workflow assets
- Let teams institutionalize QA, fix, review, governance, migration, and self-bootstrap loops
- Provide a stable harness layer above rapidly changing models and coding shells

## Continue Reading

- [01 - Quick Start](01-quickstart.md)
- [Architecture](https://github.com/c9r-io/orchestrator/blob/main/docs/architecture.md)
- [How OpenAI uses Codex](https://cdn.openai.com/pdf/6a2631dc-783e-479b-b1a4-af0cfbd38630/how-openai-uses-codex.pdf)
