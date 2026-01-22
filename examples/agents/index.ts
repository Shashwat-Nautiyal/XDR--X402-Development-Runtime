// A resilient AI Agent that knows how to pay x402
// Usage: ts-node index.ts

import axios, { AxiosError } from 'axios';
import chalk from 'chalk';

const AGENT_ID = "agent-007";
const TARGET_URL = "http://localhost:4002/post"; // XDR Proxy
const UPSTREAM = "httpbin.org";

async function runAgent() {
    console.log(chalk.cyan(`🤖 Agent ${AGENT_ID} coming online...`));
    console.log(chalk.gray(`📍 Target: ${UPSTREAM} via XDR Proxy`));

    // Simulate an agent loop
    for (let i = 1; i <= 10; i++) {
        await makeRequest(i);
        await new Promise(r => setTimeout(r, 1000)); // Pace the agent
    }
}

async function makeRequest(seq: number) {
    process.stdout.write(chalk.white(`[#${seq}] Requesting resource... `));

    try {
        // 1. Initial Request (Will likely fail with 402)
        await send(null);
        console.log(chalk.green("✅ Success (200 OK)"));
    } catch (error: any) {
        // 2. Handle x402 Payment
        if (error.response?.status === 402) {
            const invoice = error.response.data.x402_invoice;
            const cost = error.response.data.amount;
            console.log(chalk.yellow(`\n   💰 402 Payment Required: ${cost} USDC`));
            console.log(chalk.gray(`   🧾 Invoice: ${invoice}`));

            // Simulate Signing / Paying
            const token = `L402 ${invoice}`; 
            
            try {
                process.stdout.write(chalk.white(`   🔄 Retrying with Payment... `));
                await send(token);
                console.log(chalk.green("✅ Payment Accepted -> Success!"));
            } catch (retryErr: any) {
                handleFailure(retryErr);
            }
        } 
        // 3. Handle Chaos (503/429)
        else {
            handleFailure(error);
        }
    }
}

async function send(token: string | null) {
    const headers: any = {
        "X-Agent-ID": AGENT_ID,
        "X-Upstream-Host": UPSTREAM,
        "X-Simulate-Payment": "true" // Trigger XDR logic
    };
    if (token) headers["Authorization"] = token;

    await axios.post(TARGET_URL, { msg: "Hello Cronos" }, { headers });
}

function handleFailure(error: any) {
    if (error.response) {
        const s = error.response.status;
        if (s === 503 || s === 502) console.log(chalk.red(`💥 Network Error (${s}) - Retrying...`));
        else if (s === 429) console.log(chalk.red(`⏳ Rate Limited (${s}) - Backing off...`));
        else if (s === 402) console.log(chalk.magenta(`🛑 Budget Exhausted: ${JSON.stringify(error.response.data)}`));
        else console.log(chalk.red(`❌ Failed: ${s}`));
    } else {
        console.log(chalk.red(`❌ Connection Failed (Is XDR running?)`));
    }
}

runAgent();