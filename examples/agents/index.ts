// A resilient AI Agent that knows how to pay x402
// Usage: ts-node index.ts

import axios, { AxiosError } from 'axios';
import chalk from 'chalk';

const AGENT_ID = "agent-007";
const XDR_BASE = "http://localhost:4002";
const TARGET_URL = `${XDR_BASE}/post`;
const UPSTREAM = "httpbin.org";

// --- Balance & Funding Helpers ---
async function getBalance(): Promise<number | null> {
    try {
        const res = await axios.get(`${XDR_BASE}/_xdr/status/${AGENT_ID}`);
        return res.data.balance_usdc;
    } catch {
        return null;
    }
}

async function fundAgent(amount: number): Promise<boolean> {
    try {
        await axios.post(`${XDR_BASE}/_xdr/budget/${AGENT_ID}`, { amount });
        return true;
    } catch {
        return false;
    }
}

async function runAgent() {
    console.log(chalk.cyan.bold(`\n╔══════════════════════════════════════╗`));
    console.log(chalk.cyan.bold(`║   🤖 AI Agent Demo - x402 Protocol   ║`));
    console.log(chalk.cyan.bold(`╚══════════════════════════════════════╝\n`));

    // Step 1: Check if XDR is running
    console.log(chalk.white(`[1/4] Connecting to XDR Proxy...`));
    const initialBalance = await getBalance();
    
    if (initialBalance === null) {
        // Agent doesn't exist yet, pre-fund it
        console.log(chalk.yellow(`   ⚠️  Agent not found. Pre-funding with $100 USDC...`));
        const funded = await fundAgent(100);
        if (funded) {
            console.log(chalk.green(`   ✅ Agent ${AGENT_ID} funded with $100 USDC`));
        } else {
            console.log(chalk.red(`   ❌ Failed to fund agent. Is XDR running?`));
            return;
        }
    } else {
        console.log(chalk.green(`   ✅ Connected! Current balance: $${initialBalance.toFixed(2)} USDC`));
    }

    // Step 2: Show starting balance
    console.log(chalk.white(`\n[2/4] Checking wallet balance...`));
    const startBalance = await getBalance();
    console.log(chalk.blue(`   💰 Starting Balance: $${startBalance?.toFixed(2) ?? '?'} USDC`));
    console.log(chalk.gray(`   📍 Target: ${UPSTREAM} via XDR Proxy\n`));

    // Step 3: Make API requests
    console.log(chalk.white(`[3/4] Making paid API requests...\n`));
    
    for (let i = 1; i <= 5; i++) {
        await makeRequest(i);
        
        // Show balance after each request
        const currentBalance = await getBalance();
        if (currentBalance !== null) {
            console.log(chalk.gray(`       Balance: $${currentBalance.toFixed(2)} USDC\n`));
        }
        
        await new Promise(r => setTimeout(r, 1500)); // Pace for demo visibility
    }

    // Step 4: Final summary
    console.log(chalk.white(`[4/4] Session Summary`));
    const endBalance = await getBalance();
    const spent = (startBalance ?? 0) - (endBalance ?? 0);
    
    console.log(chalk.cyan(`   ┌─────────────────────────────┐`));
    console.log(chalk.cyan(`   │ Starting Balance: $${(startBalance ?? 0).toFixed(2).padStart(6)} │`));
    console.log(chalk.cyan(`   │ Ending Balance:   $${(endBalance ?? 0).toFixed(2).padStart(6)} │`));
    console.log(chalk.yellow(`   │ Total Spent:      $${spent.toFixed(2).padStart(6)} │`));
    console.log(chalk.cyan(`   └─────────────────────────────┘\n`));
}

async function makeRequest(seq: number) {
    process.stdout.write(chalk.white(`   [#${seq}] Requesting resource... `));

    try {
        await send(null);
        console.log(chalk.green("✅ Success (200 OK)"));
    } catch (error: any) {
        if (error.response?.status === 402) {
            const invoice = error.response.data.x402_invoice;
            const cost = error.response.data.amount;
            console.log(chalk.yellow(`💰 Payment Required: ${cost} USDC`));

            const token = `L402 ${invoice}`; 
            
            try {
                process.stdout.write(chalk.white(`       🔄 Paying & retrying... `));
                await send(token);
                console.log(chalk.green("✅ Paid & Success!"));
            } catch (retryErr: any) {
                handleFailure(retryErr);
            }
        } else {
            handleFailure(error);
        }
    }
}

async function send(token: string | null) {
    const headers: any = {
        "X-Agent-ID": AGENT_ID,
        "X-Upstream-Host": UPSTREAM,
        "X-Simulate-Payment": "true"
    };
    if (token) headers["Authorization"] = token;

    await axios.post(TARGET_URL, { msg: "Hello Cronos" }, { headers });
}

function handleFailure(error: any) {
    if (error.response) {
        const s = error.response.status;
        if (s === 503 || s === 502) console.log(chalk.red(`💥 Network Error (${s})`));
        else if (s === 429) console.log(chalk.red(`⏳ Rate Limited (${s})`));
        else if (s === 402) console.log(chalk.magenta(`🛑 Budget Exhausted`));
        else console.log(chalk.red(`❌ Failed: ${s}`));
    } else {
        console.log(chalk.red(`❌ Connection Failed (Is XDR running?)`));
    }
}

runAgent();