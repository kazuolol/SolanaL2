/**
 * L2 Connection - HTTP JSON-RPC + WebSocket
 */

export interface RpcResponse<T> {
  jsonrpc: string;
  id: number;
  result?: T;
  error?: { code: number; message: string };
}

export interface AccountInfo {
  data: [string, string]; // [data, encoding]
  executable: boolean;
  lamports: number;
  owner: string;
  rentEpoch: number;
}

export interface GetAccountInfoResponse {
  context: { slot: number };
  value: AccountInfo | null;
}

export interface GetLatestBlockhashResponse {
  context: { slot: number };
  value: {
    blockhash: string;
    lastValidBlockHeight: number;
  };
}

export type AccountSubscribeCallback = (pubkey: string, account: AccountInfo) => void;

export class L2Connection {
  private rpcUrl: string;
  private wsUrl: string;
  private ws: WebSocket | null = null;
  private subscriptions: Map<number, { pubkey: string; callback: AccountSubscribeCallback }> = new Map();
  private nextSubId = 1;
  private rpcId = 1;
  private onStatusChange: (status: 'connected' | 'disconnected' | 'connecting') => void;
  private pendingRequests: Map<number, { resolve: (value: any) => void; reject: (error: any) => void }> = new Map();

  constructor(
    rpcUrl: string = 'http://127.0.0.1:8899',
    wsUrl: string = 'ws://127.0.0.1:8900',
    onStatusChange?: (status: 'connected' | 'disconnected' | 'connecting') => void
  ) {
    this.rpcUrl = rpcUrl;
    this.wsUrl = wsUrl;
    this.onStatusChange = onStatusChange || (() => {});
  }

  /** Make an RPC call */
  async rpc<T>(method: string, params: any[] = []): Promise<T> {
    const id = this.rpcId++;
    const response = await fetch(this.rpcUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        jsonrpc: '2.0',
        id,
        method,
        params,
      }),
    });

    const json: RpcResponse<T> = await response.json();
    if (json.error) {
      throw new Error(`RPC Error: ${json.error.message}`);
    }
    return json.result as T;
  }

  /** Get account info */
  async getAccountInfo(pubkey: string, encoding: string = 'base64'): Promise<AccountInfo | null> {
    const response = await this.rpc<GetAccountInfoResponse>('getAccountInfo', [
      pubkey,
      { encoding },
    ]);
    return response.value;
  }

  /** Get latest blockhash */
  async getLatestBlockhash(): Promise<{ blockhash: string; lastValidBlockHeight: number }> {
    const response = await this.rpc<GetLatestBlockhashResponse>('getLatestBlockhash', []);
    return response.value;
  }

  /** Get current slot */
  async getSlot(): Promise<number> {
    return this.rpc<number>('getSlot', []);
  }

  /** Send transaction (base64 encoded) */
  async sendTransaction(txBase64: string): Promise<string> {
    return this.rpc<string>('sendTransaction', [txBase64]);
  }

  /** Connect WebSocket */
  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.onStatusChange('connecting');

      this.ws = new WebSocket(this.wsUrl);

      this.ws.onopen = () => {
        console.log('[WS] Connected to', this.wsUrl);
        this.onStatusChange('connected');
        resolve();
      };

      this.ws.onclose = () => {
        console.log('[WS] Disconnected');
        this.onStatusChange('disconnected');
        // Attempt reconnect after 2 seconds
        setTimeout(() => {
          if (this.subscriptions.size > 0) {
            this.connect().then(() => {
              // Resubscribe
              for (const [_, sub] of this.subscriptions) {
                this.sendSubscribe(sub.pubkey);
              }
            }).catch(console.error);
          }
        }, 2000);
      };

      this.ws.onerror = (error) => {
        console.error('[WS] Error:', error);
        reject(error);
      };

      this.ws.onmessage = (event) => {
        this.handleWsMessage(event.data);
      };
    });
  }

  /** Handle WebSocket message */
  private handleWsMessage(data: string) {
    try {
      const msg = JSON.parse(data);

      // Handle subscription response
      if (msg.id && this.pendingRequests.has(msg.id)) {
        const { resolve, reject } = this.pendingRequests.get(msg.id)!;
        this.pendingRequests.delete(msg.id);
        if (msg.error) {
          reject(new Error(msg.error.message));
        } else {
          resolve(msg.result);
        }
        return;
      }

      // Handle account notification
      if (msg.method === 'accountNotification') {
        const { subscription, result } = msg.params;
        const sub = this.subscriptions.get(subscription);
        if (sub && result.value) {
          sub.callback(sub.pubkey, result.value);
        }
      }
    } catch (e) {
      console.error('[WS] Failed to parse message:', e);
    }
  }

  /** Send subscribe request */
  private sendSubscribe(pubkey: string): Promise<number> {
    return new Promise((resolve, reject) => {
      const id = this.rpcId++;
      this.pendingRequests.set(id, { resolve, reject });

      this.ws?.send(JSON.stringify({
        jsonrpc: '2.0',
        id,
        method: 'accountSubscribe',
        params: [pubkey, { encoding: 'base64' }],
      }));

      // Timeout after 5 seconds
      setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error('Subscribe timeout'));
        }
      }, 5000);
    });
  }

  /** Subscribe to account updates */
  async accountSubscribe(pubkey: string, callback: AccountSubscribeCallback): Promise<number> {
    const subId = this.nextSubId++;

    this.subscriptions.set(subId, { pubkey, callback });

    if (this.ws?.readyState === WebSocket.OPEN) {
      await this.sendSubscribe(pubkey);
    }

    return subId;
  }

  /** Unsubscribe from account updates */
  accountUnsubscribe(subId: number): void {
    this.subscriptions.delete(subId);
  }

  /** Disconnect WebSocket */
  disconnect(): void {
    this.ws?.close();
    this.ws = null;
    this.subscriptions.clear();
  }
}
