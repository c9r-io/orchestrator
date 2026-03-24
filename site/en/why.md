# Why Agent Orchestrator?

The Agent Orchestrator is purpose-built for **AI-native software development lifecycle automation**. Unlike general-purpose workflow engines, it treats AI agents as first-class participants in the development process.

## Comparison

| Feature | Agent Orchestrator | Airflow | Prefect | n8n | Dagger |
|---------|-------------------|---------|---------|-----|--------|
| **Primary focus** | AI-native SDLC automation | Data pipeline scheduling | Data workflow orchestration | Low-code automation | CI/CD pipelines |
| **Agent orchestration** | Built-in: capability matching, health scoring, rotation | Not applicable | Not applicable | Not applicable | Not applicable |
| **Control flow** | CEL expressions (Run/Skip/Branch/DynamicAdd/Transform) | Python DAGs | Python decorators | Visual flow editor | Go/Python SDK |
| **Security model** | mTLS + RBAC + sandbox (Seatbelt/namespaces) + output redaction | Connection-level auth | API keys | Basic auth | Container isolation |
| **Deployment** | Single binary + embedded SQLite | Scheduler + workers + metadata DB | Server + workers + DB | Server + DB | Container engine |
| **Configuration** | Declarative YAML manifests | Python code | Python code | JSON (visual editor) | Go/Python code |
| **Built for AI agents** | Yes — spawn, monitor, score, rotate agents | No | No | No | No |

## Key Differentiators

### AI Agent as First-Class Citizen

Steps declare `required_capability`, agents declare `capabilities`. The orchestrator matches them automatically with health-aware scoring and rotation.

```yaml
kind: Agent
metadata:
  name: tester
spec:
  capabilities: [qa_testing]
  command: claude -p "{prompt}" --verbose
```

### CEL-Powered Dynamic Control Flow

Runtime decisions via Common Expression Language — no code changes needed.

```yaml
prehook:
  expression: |
    pipeline.step_outputs["scan"].exit_code == 0
      ? "run"
      : "skip"
```

### Declarative, Not Imperative

Everything is YAML manifests applied via `orchestrator apply -f`. No Python, no Go, no SDK lock-in.

### Single-Binary Deployment

`orchestratord` is a single Rust binary with embedded SQLite. No external database, no message queue, no container runtime required.

### Built-in Security

- **mTLS**: Mutual TLS for all daemon communication
- **RBAC**: Role-based access control on gRPC endpoints
- **Sandbox**: macOS Seatbelt profiles or Linux namespace isolation
- **Redaction**: Automatic secret/token/password filtering in logs
