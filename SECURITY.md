# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 1.0.x   | ✅ |
| < 1.0   | ❌ |

## Reporting a Vulnerability

Please report security vulnerabilities privately to the maintainers.
Do NOT open a public issue.

We aim to acknowledge reports within 48 hours and provide a fix timeline
within 7 days of confirmation.

## Security Architecture

Candor AI enforces a deny-by-default security posture:

- **Sandbox isolation**: All tool execution runs inside WASM (wasmtime) or OS-level sandboxes (bubblewrap/Seatbelt) with network denied by default
- **Sentinel guardrails**: A sidecar interceptor evaluates all agent actions against deterministic rules before execution
- **Git discipline**: Force-push and destructive operations are mechanically blocked
- **No secrets in code**: API keys are loaded exclusively from environment variables — never committed
- **Operational doctrine**: 10 Lawful Good principles encoded as runtime guardrails

## Best Practices for Deployment

1. Set API keys via environment variables, not config files:
   ```bash
   export OPENAI_API_KEY="sk-..."    # or ANTHROPIC_API_KEY
   export LM_STUDIO_URL="http://localhost:1234/v1"  # for local models
   ```

2. Run the sandbox with bubblewrap on Linux for process isolation:
   ```bash
   sudo apt install bubblewrap  # or equivalent
   ```

3. Keep the `.env` file permissions restricted:
   ```bash
   chmod 600 .env
   ```

4. Review `candor.toml` before deployment — uncomment only the providers you need.
