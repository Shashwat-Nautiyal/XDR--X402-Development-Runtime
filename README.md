# XDR: The Cronos Agent Foundry ⚒️
XDR lets developers test agent economics, payment semantics, and failure modes before touching real money or real users.

**Local-first runtime, simulator, and debugger for x402-powered AI agents on Cronos.**

[![Cronos](https://img.shields.io/badge/Built%20For-Cronos%20zkEVM-blue)](https://cronos.org/)
[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange)](https://www.rust-lang.org/)

---

## 🚧 The Problem
Building autonomous agents that pay for resources (LLMs, APIs, Data) is dangerous and slow:
1.  **Cost:** Testing requires burning real testnet/mainnet USDC.
2.  **Risk:** A buggy loop can drain a wallet in seconds.
3.  **Blindness:** You can't verify how your agent handles "Payment Required" (402) errors without a live paywall.

## 🚀 The Solution: XDR
XDR is a **Reverse Proxy & Runtime** that sits between your agent and the world. It mocks the entire **Cronos x402 Payment Lifecycle** locally.

### ✨ Key Features

* **🌍 Cronos Network Simulation:**
    * Mocks Chain ID `338` (Testnet) or `25` (Mainnet).
    * Generates deterministic Transaction Hashes for every payment.
    * Standardized `L402` Payment Challenges.

* **🌪️ Deterministic Chaos Engine:**
    * Inject "Rug Pulls" (Payment success -> Request failure).
    * Simulate Mempool Congestion (Latency).
    * Simulate RPC Node drops (503 Service Unavailable).
    * **Seedable:** Replay exact failure scenarios to debug agent logic.

* **💸 Budget Enforcement:**
    * Set hard spending caps (e.g., "$5.00 USDC").
    * XDR blocks requests with `402 Budget Exceeded` when the cap is hit.
    * Protects you from runaway loops during development.

---

## ⚡ Quick Start

### 1. Install & Run
```bash
# Clone and run the single binary
cargo run -- run --network cronos-testnet