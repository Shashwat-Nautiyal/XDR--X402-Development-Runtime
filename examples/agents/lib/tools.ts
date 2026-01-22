/**
 * Tool Definitions for Gemini Agent
 * 
 * These tools are available for the AI agent to call.
 * Each tool makes requests through the XDR proxy.
 */

import { FunctionDeclaration, SchemaType } from '@google/generative-ai';
import { XDRClient, XDRResponse } from './xdr-client';
import chalk from 'chalk';

// --- Tool Declarations for Gemini ---
// These tell Gemini what tools are available and their parameters

export const toolDeclarations: FunctionDeclaration[] = [
    {
        name: 'http_request',
        description: `Make an HTTP request to an external API. All requests are routed through the XDR payment proxy. 
                      Use this to fetch data from APIs, call web services, or interact with external resources.
                      The agent will automatically handle payment (402) challenges.`,
        parameters: {
            type: SchemaType.OBJECT,
            properties: {
                method: {
                    type: SchemaType.STRING,
                    description: 'HTTP method: GET or POST',
                    enum: ['GET', 'POST']
                },
                host: {
                    type: SchemaType.STRING,
                    description: 'The target host/domain (e.g., "httpbin.org", "api.example.com")'
                },
                path: {
                    type: SchemaType.STRING,
                    description: 'The URL path (e.g., "/get", "/api/v1/users")'
                },
                body: {
                    type: SchemaType.STRING,
                    description: 'Optional JSON body for POST requests (as a string)'
                }
            },
            required: ['method', 'host', 'path']
        }
    },
    {
        name: 'get_agent_balance',
        description: 'Check the current USDC balance and spending stats for this agent in the XDR ledger.',
        parameters: {
            type: SchemaType.OBJECT,
            properties: {},
            required: []
        }
    }
];

// --- Tool Executor ---
// Maps tool names to actual implementations

export class ToolExecutor {
    private xdrClient: XDRClient;
    private agentId: string;
    private proxyUrl: string;

    constructor(xdrClient: XDRClient, agentId: string, proxyUrl: string) {
        this.xdrClient = xdrClient;
        this.agentId = agentId;
        this.proxyUrl = proxyUrl;
    }

    /**
     * Execute a tool by name with given arguments
     */
    async execute(toolName: string, args: Record<string, any>): Promise<string> {
        console.log(chalk.cyan(`\n🔧 Executing tool: ${toolName}`));
        console.log(chalk.gray(`   Args: ${JSON.stringify(args)}`));

        switch (toolName) {
            case 'http_request':
                return this.httpRequest(args);
            case 'get_agent_balance':
                return this.getAgentBalance();
            default:
                return JSON.stringify({ error: `Unknown tool: ${toolName}` });
        }
    }

    /**
     * Tool: http_request
     * Makes an HTTP request through XDR proxy
     */
    private async httpRequest(args: Record<string, any>): Promise<string> {
        const { method, host, path, body } = args;

        let parsedBody: any = undefined;
        if (body) {
            try {
                parsedBody = JSON.parse(body);
            } catch {
                parsedBody = { raw: body };
            }
        }

        let response: XDRResponse;
        
        if (method === 'GET') {
            response = await this.xdrClient.get(host, path);
        } else {
            response = await this.xdrClient.post(host, path, parsedBody);
        }

        // Format response for the LLM
        if (response.success) {
            const result = {
                success: true,
                status: response.status,
                data: response.data,
                payment: response.paymentMade ? {
                    made: true,
                    amount: response.amountPaid
                } : { made: false }
            };
            console.log(chalk.green(`   ✅ Request succeeded (${response.status})`));
            return JSON.stringify(result, null, 2);
        } else {
            const result = {
                success: false,
                status: response.status,
                error: response.error,
                payment: response.paymentMade ? {
                    made: true,
                    amount: response.amountPaid
                } : { made: false }
            };
            console.log(chalk.red(`   ❌ Request failed: ${response.error}`));
            return JSON.stringify(result, null, 2);
        }
    }

    /**
     * Tool: get_agent_balance
     * Queries XDR ledger for agent's current balance
     */
    private async getAgentBalance(): Promise<string> {
        try {
            const response = await fetch(`${this.proxyUrl}/_xdr/status/${this.agentId}`);
            
            if (response.ok) {
                // AgentState fields from xdr-ledger: balance_usdc, total_spend, payment_count
                const data = await response.json() as { balance_usdc: number; total_spend: number; payment_count: number };
                console.log(chalk.green(`   ✅ Balance: $${data.balance_usdc} USDC`));
                return JSON.stringify({
                    success: true,
                    agent_id: this.agentId,
                    balance_usdc: data.balance_usdc,
                    total_spend: data.total_spend,
                    payment_count: data.payment_count
                }, null, 2);
            } else if (response.status === 404) {
                return JSON.stringify({
                    success: true,
                    agent_id: this.agentId,
                    balance: 'Not registered yet (make a request first)',
                    note: 'Agent will be registered on first request through XDR'
                }, null, 2);
            } else {
                return JSON.stringify({
                    success: false,
                    error: `Failed to get balance: ${response.status}`
                }, null, 2);
            }
        } catch (error) {
            return JSON.stringify({
                success: false,
                error: 'Could not connect to XDR. Is it running?'
            }, null, 2);
        }
    }
}

export default ToolExecutor;
