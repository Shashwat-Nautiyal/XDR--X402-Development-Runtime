/**
 * Gemini AI Agent for XDR
 * 
 * A ReAct-style agent that uses Gemini for reasoning and makes
 * external API calls through the XDR payment proxy.
 * 
 * Usage:
 *   1. Copy .env.example to .env and add your GEMINI_API_KEY
 *   2. Start XDR: cargo run -- run --network cronos-testnet
 *   3. Run agent: npx ts-node gemini-agent.ts
 */

import { GoogleGenerativeAI, Content, Part, FunctionCall } from '@google/generative-ai';
import * as dotenv from 'dotenv';
import chalk from 'chalk';
import * as readline from 'readline';

import { XDRClient } from './lib/xdr-client';
import { ToolExecutor, toolDeclarations } from './lib/tools';

// Load environment variables
dotenv.config();

// --- Configuration ---
const CONFIG = {
    geminiApiKey: process.env.GEMINI_API_KEY || '',
    geminiModel: 'gemini-2.5-flash',  // Fast model for demos
    xdrProxyUrl: process.env.XDR_PROXY_URL || 'http://localhost:4002',
    agentId: process.env.AGENT_ID || 'gemini-agent-01',
    maxTurns: 10,  // Safety limit on agent loop iterations
};

// --- System Prompt ---
const SYSTEM_PROMPT = `You are an autonomous AI agent that can interact with external APIs to accomplish tasks.

IMPORTANT CONTEXT:
- All your HTTP requests go through the XDR payment proxy
- Some APIs require payment - you will see payment confirmations in tool responses
- You have a limited USDC budget - be mindful of costs
- If you see "Budget Exhausted", stop making requests

AVAILABLE TOOLS:
1. http_request: Make HTTP GET/POST requests to any API
2. get_agent_balance: Check your current USDC balance

BEHAVIOR:
- Think step by step about how to accomplish the user's goal
- Use tools when you need external data
- Summarize results clearly for the user
- If a request fails, explain what went wrong`;

// --- Agent Class ---
class GeminiAgent {
    private genAI: GoogleGenerativeAI;
    private model: any;
    private xdrClient: XDRClient;
    private toolExecutor: ToolExecutor;
    private conversationHistory: Content[] = [];

    constructor() {
        if (!CONFIG.geminiApiKey) {
            throw new Error('GEMINI_API_KEY not found! Copy .env.example to .env and add your key.');
        }

        // Initialize Gemini
        this.genAI = new GoogleGenerativeAI(CONFIG.geminiApiKey);
        this.model = this.genAI.getGenerativeModel({
            model: CONFIG.geminiModel,
            tools: [{ functionDeclarations: toolDeclarations }],
            systemInstruction: SYSTEM_PROMPT,
        });

        // Initialize XDR Client
        this.xdrClient = new XDRClient({
            proxyUrl: CONFIG.xdrProxyUrl,
            agentId: CONFIG.agentId,
        });

        // Initialize Tool Executor
        this.toolExecutor = new ToolExecutor(
            this.xdrClient,
            CONFIG.agentId,
            CONFIG.xdrProxyUrl
        );
    }

    /**
     * Process a user message and return the agent's response
     * Handles the full ReAct loop: Think → Act → Observe → Repeat
     */
    async processMessage(userMessage: string): Promise<string> {
        console.log(chalk.cyan(`\n🤖 Agent thinking...`));

        // Add user message to history
        this.conversationHistory.push({
            role: 'user',
            parts: [{ text: userMessage }]
        });

        let turn = 0;

        while (turn < CONFIG.maxTurns) {
            turn++;
            console.log(chalk.gray(`\n--- Turn ${turn}/${CONFIG.maxTurns} ---`));

            // Get response from Gemini
            const chat = this.model.startChat({
                history: this.conversationHistory.slice(0, -1), // All but last message
            });

            const lastMessage = this.conversationHistory[this.conversationHistory.length - 1];
            const result = await chat.sendMessage(lastMessage.parts);
            const response = result.response;

            // Check for function calls
            const functionCalls = response.functionCalls();

            if (functionCalls && functionCalls.length > 0) {
                // Agent wants to use a tool
                console.log(chalk.yellow(`\n🧠 Agent decided to call ${functionCalls.length} tool(s)`));

                // Add model's response to history
                this.conversationHistory.push({
                    role: 'model',
                    parts: response.candidates[0].content.parts
                });

                // Execute each function call and collect results
                const functionResponses: Part[] = [];

                for (const fc of functionCalls) {
                    const toolResult = await this.toolExecutor.execute(
                        fc.name,
                        fc.args as Record<string, any>
                    );

                    functionResponses.push({
                        functionResponse: {
                            name: fc.name,
                            response: { result: toolResult }
                        }
                    });
                }

                // Add function results to history using 'function' role for proper Gemini format
                this.conversationHistory.push({
                    role: 'function' as any,  // Gemini expects 'function' role for function responses
                    parts: functionResponses
                });

                // Continue the loop - agent will process the tool results
                continue;
            }

            // No function calls - agent is done, return text response
            const textResponse = response.text();
            
            // Add final response to history
            this.conversationHistory.push({
                role: 'model',
                parts: [{ text: textResponse }]
            });

            return textResponse;
        }

        return "I've reached my turn limit. Please try a simpler request.";
    }

    /**
     * Interactive chat loop
     */
    async runInteractive(): Promise<void> {
        console.log(chalk.bold.cyan('\n╔═══════════════════════════════════════════════════════════╗'));
        console.log(chalk.bold.cyan('║       🤖 Gemini Agent for XDR - Interactive Mode          ║'));
        console.log(chalk.bold.cyan('╚═══════════════════════════════════════════════════════════╝'));
        console.log(chalk.gray(`\nAgent ID: ${CONFIG.agentId}`));
        console.log(chalk.gray(`XDR Proxy: ${CONFIG.xdrProxyUrl}`));
        console.log(chalk.gray(`Model: ${CONFIG.geminiModel}`));
        console.log(chalk.yellow('\nType your requests, or "quit" to exit.\n'));

        const rl = readline.createInterface({
            input: process.stdin,
            output: process.stdout
        });

        const askQuestion = (): void => {
            rl.question(chalk.green('You: '), async (input) => {
                const trimmed = input.trim();
                
                if (trimmed.toLowerCase() === 'quit' || trimmed.toLowerCase() === 'exit') {
                    console.log(chalk.cyan('\n👋 Agent shutting down. Goodbye!\n'));
                    rl.close();
                    return;
                }

                if (!trimmed) {
                    askQuestion();
                    return;
                }

                try {
                    const response = await this.processMessage(trimmed);
                    console.log(chalk.cyan(`\n🤖 Agent: ${response}\n`));
                } catch (error: any) {
                    console.log(chalk.red(`\n❌ Error: ${error.message}\n`));
                }

                askQuestion();
            });
        };

        askQuestion();
    }

    /**
     * Run a single demo task
     */
    async runDemo(): Promise<void> {
        console.log(chalk.bold.cyan('\n╔═══════════════════════════════════════════════════════════╗'));
        console.log(chalk.bold.cyan('║         🤖 Gemini Agent for XDR - Demo Mode               ║'));
        console.log(chalk.bold.cyan('╚═══════════════════════════════════════════════════════════╝'));
        console.log(chalk.gray(`\nAgent ID: ${CONFIG.agentId}`));
        console.log(chalk.gray(`XDR Proxy: ${CONFIG.xdrProxyUrl}`));
        console.log(chalk.gray('─'.repeat(60)));

        // ========================================
        // STAGE 1: Fund the Agent
        // ========================================
        console.log(chalk.bold.yellow('\n📍 STAGE 1: Funding the Agent'));
        console.log(chalk.gray('─'.repeat(60)));
        
        const initialBudget = 5.0; // $5 USDC
        console.log(chalk.white(`Setting budget to $${initialBudget} USDC...`));
        
        try {
            const fundRes = await fetch(`${CONFIG.xdrProxyUrl}/_xdr/budget/${CONFIG.agentId}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ amount: initialBudget })
            });
            if (fundRes.ok) {
                console.log(chalk.green(`✅ Agent funded with $${initialBudget} USDC\n`));
            } else {
                console.log(chalk.red(`❌ Failed to fund agent: ${fundRes.status}`));
                return;
            }
        } catch (e) {
            console.log(chalk.red(`❌ Cannot connect to XDR. Is it running on ${CONFIG.xdrProxyUrl}?`));
            return;
        }

        // Check balance
        const balanceRes = await fetch(`${CONFIG.xdrProxyUrl}/_xdr/status/${CONFIG.agentId}`);
        if (balanceRes.ok) {
            // AgentState fields from xdr-ledger: balance_usdc, total_spend, payment_count
            const status = await balanceRes.json() as { balance_usdc: number };
            console.log(chalk.cyan(`💰 Current Balance: $${status.balance_usdc} USDC`));
        }

        await this.pause(1000);

        // ========================================
        // STAGE 2: x402 Payment Flow
        // ========================================
        console.log(chalk.bold.yellow('\n📍 STAGE 2: x402 Payment Flow'));
        console.log(chalk.gray('─'.repeat(60)));
        console.log(chalk.white('Making an API request through XDR...'));
        console.log(chalk.gray('Watch for the 402 Payment Required → Payment → Success flow\n'));

        try {
            const response = await this.processMessage(
                "Make a GET request to httpbin.org at the /ip endpoint to get my public IP address."
            );
            console.log(chalk.bold.cyan('\n🤖 Agent Response:'));
            console.log(chalk.white(response));
        } catch (error: any) {
            console.log(chalk.red(`❌ Error: ${error.message}`));
        }

        // Show updated balance
        const afterPayment = await fetch(`${CONFIG.xdrProxyUrl}/_xdr/status/${CONFIG.agentId}`);
        if (afterPayment.ok) {
            const status = await afterPayment.json() as { balance_usdc: number; total_spend: number };
            console.log(chalk.cyan(`\n💰 Balance after payment: $${status.balance_usdc} USDC (spent: $${status.total_spend})`));
        }

        await this.pause(2000);

        // ========================================
        // STAGE 3: Chaos Testing
        // ========================================
        console.log(chalk.bold.yellow('\n📍 STAGE 3: Chaos Engineering'));
        console.log(chalk.gray('─'.repeat(60)));
        console.log(chalk.white('Enabling chaos: 30% failure rate, 500ms latency...\n'));

        // Enable chaos via XDR API
        try {
            await fetch(`${CONFIG.xdrProxyUrl}/_xdr/chaos`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({
                    enabled: true,
                    seed: 42,
                    failure_rate: 0.3,
                    min_latency_ms: 500,
                    max_latency_ms: 1000
                })
            });
            console.log(chalk.magenta('🌪️  Chaos enabled! Agent must handle failures gracefully.\n'));
        } catch (e) {
            console.log(chalk.yellow('⚠️  Could not enable chaos (endpoint may not exist yet)'));
        }

        // Reset conversation for fresh context
        this.conversationHistory = [];

        try {
            const response = await this.processMessage(
                "Make a POST request to httpbin.org at /post with a JSON body containing {\"test\": \"chaos\"}. Report if it succeeds or fails."
            );
            console.log(chalk.bold.cyan('\n🤖 Agent Response:'));
            console.log(chalk.white(response));
        } catch (error: any) {
            console.log(chalk.red(`❌ Error: ${error.message}`));
        }

        // Disable chaos
        try {
            await fetch(`${CONFIG.xdrProxyUrl}/_xdr/chaos`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ enabled: false })
            });
            console.log(chalk.gray('\n🌪️  Chaos disabled.'));
        } catch (e) { /* ignore */ }

        await this.pause(2000);

        // ========================================
        // STAGE 4: Budget Exhaustion
        // ========================================
        console.log(chalk.bold.yellow('\n📍 STAGE 4: Budget Exhaustion'));
        console.log(chalk.gray('─'.repeat(60)));
        
        // Set a small budget - enough for 2 requests, but we'll ask for 5
        // Each request costs $0.01 (set by XDR proxy), so $0.02 allows 2 requests
        const exhaustionBudget = 0.02;
        console.log(chalk.white(`Setting budget to $${exhaustionBudget} USDC (enough for 2 requests at $0.01 each)...`));
        console.log(chalk.gray(`We'll ask the agent to make 5 requests - it should fail after 2.\n`));
        
        await fetch(`${CONFIG.xdrProxyUrl}/_xdr/budget/${CONFIG.agentId}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ amount: exhaustionBudget })
        });

        // Reset conversation
        this.conversationHistory = [];

        try {
            const response = await this.processMessage(
                "Make 5 consecutive GET requests to httpbin.org/uuid to get unique IDs. Report each UUID you receive, and tell me if any requests fail."
            );
            console.log(chalk.bold.cyan('\n🤖 Agent Response:'));
            console.log(chalk.white(response));
        } catch (error: any) {
            // Expected! Budget exhaustion causes tool failures which can confuse Gemini
            console.log(chalk.magenta(`\n🛑 Agent stopped: Budget exhausted (this is expected behavior)`));
            console.log(chalk.gray(`   The agent was blocked from making further paid requests.`));
        }

        // Final status
        const finalStatus = await fetch(`${CONFIG.xdrProxyUrl}/_xdr/status/${CONFIG.agentId}`);
        if (finalStatus.ok) {
            const status = await finalStatus.json() as { balance_usdc: number; total_spend: number; payment_count: number };
            console.log(chalk.cyan(`\n💰 Final Status:`));
            console.log(chalk.gray(`   Balance: $${status.balance_usdc} USDC`));
            console.log(chalk.gray(`   Total Spent: $${status.total_spend} USDC`));
            console.log(chalk.gray(`   Payments Made: ${status.payment_count}`));
        }

        console.log(chalk.gray('\n' + '─'.repeat(60)));
        console.log(chalk.bold.green('\n✅ Demo complete! The agent demonstrated:'));
        console.log(chalk.white('   1. Agent funding via XDR ledger'));
        console.log(chalk.white('   2. x402 payment flow (402 → pay → retry)'));
        console.log(chalk.white('   3. Chaos resilience (handling failures)'));
        console.log(chalk.white('   4. Budget exhaustion protection\n'));
    }

    private pause(ms: number): Promise<void> {
        return new Promise(resolve => setTimeout(resolve, ms));
    }
}

// --- Main Entry Point ---
async function main() {
    const args = process.argv.slice(2);
    const mode = args[0] || 'demo';

    try {
        const agent = new GeminiAgent();

        if (mode === 'interactive' || mode === '-i') {
            await agent.runInteractive();
        } else {
            await agent.runDemo();
        }
    } catch (error: any) {
        console.error(chalk.red(`\n❌ Fatal Error: ${error.message}`));
        console.error(chalk.gray('\nTroubleshooting:'));
        console.error(chalk.gray('  1. Make sure you have a .env file with GEMINI_API_KEY'));
        console.error(chalk.gray('  2. Make sure XDR is running: cargo run -- run'));
        console.error(chalk.gray('  3. Check your Gemini API key is valid'));
        process.exit(1);
    }
}

main();
