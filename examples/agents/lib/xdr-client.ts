/**
 * XDR Client - HTTP wrapper that speaks the XDR protocol
 * 
 * Handles:
 * - Required XDR headers (X-Agent-ID, X-Upstream-Host, X-Simulate-Payment)
 * - 402 Payment Required flow (extract invoice, retry with Authorization)
 * - Chaos resilience (retry on 503/502, backoff on 429)
 */

import axios, { AxiosError, AxiosRequestConfig, AxiosResponse } from 'axios';
import chalk from 'chalk';

// --- Configuration ---
export interface XDRClientConfig {
    proxyUrl: string;      // XDR proxy URL (default: http://localhost:4002)
    agentId: string;       // Agent identity for ledger tracking
    maxRetries?: number;   // Max retries for transient failures
    baseDelay?: number;    // Base delay for exponential backoff (ms)
}

// --- Response Types ---
export interface XDRResponse<T = any> {
    success: boolean;
    data?: T;
    status: number;
    paymentMade?: boolean;
    amountPaid?: number;
    error?: string;
}

interface PaymentChallenge {
    x402_invoice: string;
    amount: number;
    currency: string;
    recipient: string;
}

// --- XDR Client Class ---
export class XDRClient {
    private proxyUrl: string;
    private agentId: string;
    private maxRetries: number;
    private baseDelay: number;

    constructor(config: XDRClientConfig) {
        this.proxyUrl = config.proxyUrl || 'http://localhost:4002';
        this.agentId = config.agentId;
        this.maxRetries = config.maxRetries ?? 3;
        this.baseDelay = config.baseDelay ?? 1000;
    }

    /**
     * Make a request through XDR proxy
     * Automatically handles 402 payment flow and chaos resilience
     */
    async request<T = any>(
        method: 'GET' | 'POST' | 'PUT' | 'DELETE',
        upstreamHost: string,
        path: string,
        data?: any,
        headers?: Record<string, string>
    ): Promise<XDRResponse<T>> {
        const url = `${this.proxyUrl}${path}`;
        
        // Build XDR-required headers
        const xdrHeaders: Record<string, string> = {
            'X-Agent-ID': this.agentId,
            'X-Upstream-Host': upstreamHost,
            'X-Simulate-Payment': 'true',
            ...headers
        };

        let attempt = 0;
        let lastError: Error | null = null;

        while (attempt < this.maxRetries) {
            attempt++;

            try {
                const response = await this.makeRequest<T>(method, url, data, xdrHeaders);
                return {
                    success: true,
                    data: response.data,
                    status: response.status,
                    paymentMade: false
                };
            } catch (error) {
                const axiosError = error as AxiosError;
                
                if (!axiosError.response) {
                    // Network error - XDR might not be running
                    console.log(chalk.red(`   ❌ Connection failed (Is XDR running on ${this.proxyUrl}?)`));
                    return {
                        success: false,
                        status: 0,
                        error: 'Connection failed - XDR proxy not reachable'
                    };
                }

                const status = axiosError.response.status;

                // --- Handle 402 Payment Required ---
                if (status === 402) {
                    const challenge = axiosError.response.data as PaymentChallenge;
                    
                    // Check if budget exhausted (different from normal 402)
                    if ((axiosError.response.data as any)?.error?.includes('Budget')) {
                        console.log(chalk.magenta(`   🛑 Budget Exhausted!`));
                        return {
                            success: false,
                            status: 402,
                            error: 'Budget exhausted'
                        };
                    }

                    console.log(chalk.yellow(`   💰 402 Payment Required: ${challenge.amount} ${challenge.currency || 'USDC'}`));
                    
                    // Simulate payment by adding Authorization header
                    const paymentResult = await this.handlePayment<T>(
                        method, url, data, xdrHeaders, challenge
                    );
                    
                    return paymentResult as XDRResponse<T>;
                }

                // --- Handle Chaos: Transient Failures ---
                if (status === 503 || status === 502) {
                    const delay = this.baseDelay * Math.pow(2, attempt - 1);
                    console.log(chalk.red(`   💥 ${status} Service Error - Retry ${attempt}/${this.maxRetries} in ${delay}ms`));
                    await this.sleep(delay);
                    lastError = axiosError;
                    continue;
                }

                // --- Handle Chaos: Rate Limiting ---
                if (status === 429) {
                    const delay = this.baseDelay * Math.pow(2, attempt);
                    console.log(chalk.yellow(`   ⏳ 429 Rate Limited - Backing off ${delay}ms`));
                    await this.sleep(delay);
                    lastError = axiosError;
                    continue;
                }

                // --- Other errors: Don't retry ---
                return {
                    success: false,
                    status,
                    error: `HTTP ${status}: ${JSON.stringify(axiosError.response.data)}`
                };
            }
        }

        // Exhausted retries
        return {
            success: false,
            status: 503,
            error: `Max retries (${this.maxRetries}) exhausted: ${lastError?.message}`
        };
    }

    /**
     * Handle 402 Payment Required - simulate signing and retry
     */
    private async handlePayment<T>(
        method: 'GET' | 'POST' | 'PUT' | 'DELETE',
        url: string,
        data: any,
        headers: Record<string, string>,
        challenge: PaymentChallenge
    ): Promise<XDRResponse<T>> {
        console.log(chalk.gray(`   🧾 Invoice: ${challenge.x402_invoice}`));
        
        // Simulate "signing" the payment (in real x402, this would be a crypto signature)
        const paymentToken = `L402 ${challenge.x402_invoice}`;
        
        const paidHeaders = {
            ...headers,
            'Authorization': paymentToken
        };

        try {
            console.log(chalk.white(`   🔄 Retrying with payment...`));
            const response = await this.makeRequest<T>(method, url, data, paidHeaders);
            console.log(chalk.green(`   ✅ Payment accepted!`));
            
            return {
                success: true,
                data: response.data,
                status: response.status,
                paymentMade: true,
                amountPaid: challenge.amount
            };
        } catch (retryError) {
            const axiosError = retryError as AxiosError;
            const status = axiosError.response?.status || 500;
            
            // The Rug Pull - payment accepted but request still failed
            if (status >= 500) {
                console.log(chalk.red(`   🔥 RUG PULL! Payment taken but request failed!`));
            }
            
            return {
                success: false,
                status,
                paymentMade: true,
                amountPaid: challenge.amount,
                error: `Payment made but request failed: ${status}`
            };
        }
    }

    /**
     * Low-level request helper
     */
    private async makeRequest<T>(
        method: string,
        url: string,
        data: any,
        headers: Record<string, string>
    ): Promise<AxiosResponse<T>> {
        const config: AxiosRequestConfig = {
            method,
            url,
            headers,
            data,
            timeout: 30000
        };

        return axios(config);
    }

    private sleep(ms: number): Promise<void> {
        return new Promise(resolve => setTimeout(resolve, ms));
    }

    // --- Convenience Methods ---

    async get<T = any>(upstreamHost: string, path: string, headers?: Record<string, string>): Promise<XDRResponse<T>> {
        return this.request('GET', upstreamHost, path, undefined, headers) as Promise<XDRResponse<T>>;
    }

    async post<T = any>(upstreamHost: string, path: string, data?: any, headers?: Record<string, string>): Promise<XDRResponse<T>> {
        return this.request('POST', upstreamHost, path, data, headers) as Promise<XDRResponse<T>>;
    }
}

export default XDRClient;
