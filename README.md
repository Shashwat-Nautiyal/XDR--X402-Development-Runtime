# XDR (x402 Dev Runtime)
**The "Foundry" for Agent Economics on Cronos.**

> 🚧 **Problem:** Building x402-powered AI agents is hard because testing payments requires real wallets, real delays, and non-deterministic failures.
>
> 🚀 **Solution:** XDR is a local-first runtime that simulates the entire Cronos x402 payment lifecycle, enforcing budgets and injecting chaos deterministicly.

---

## ⚡ Features

### 1. Deterministic x402 Simulator
Stop wasting testnet funds. XDR intercepts traffic and mocks the entire **HTTP 402 Payment Required** handshake locally.
- **Auto-Invoicing:** Generates valid `L402` challenges for any request.
- **Ledger Tracking:** Tracks every "Virtual USDC" spent by your agents.

### 2. Chaos Engineering (The "Rug" Test)
Agents on blockchain networks must survive instability. XDR acts as a hostile network condition generator.
- **Latency Injection:** Simulate mempool congestion (e.g., random 500ms - 2s delays).
- **Failure Injection:** Randomly drop requests (`503`, `429`) to test agent retry logic.
- **Rug Pull Mode:** Simulate the worst-case scenario: Payment accepted, but service returns 500.

### 3. Budget Enforcement
Prevent runaway agents from draining wallets.
- **Hard Stops:** Automatically blocks requests when `total_spend > budget`.
- **Admin API:** Update budgets on the fly without restarting the agent.

### 4. Zero-Code Integration
Works with **any** language (TypeScript, Python, Go, Rust).
- **No SDK Required:** Just change your agent's `BASE_URL`.
- **Standard Headers:** Uses `X-Agent-ID` and `Authorization: L402 ...`.

---

## 🛠️ Usage

### 1. Start the Runtime
```bash
# Starts XDR on http://localhost:4002
cargo run -- run