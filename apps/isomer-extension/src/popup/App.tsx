import { useState, useEffect } from 'react';
import { Wallet, Copy, Check, Droplets, RefreshCw, Settings, ExternalLink } from 'lucide-react';

interface WalletState {
    initialized: boolean;
    unlocked: boolean;
    hasWallet: boolean;
    address?: string;
    balance?: number;
}

function App() {
    const [state, setState] = useState<WalletState | null>(null);
    const [balance, setBalance] = useState<number>(0);
    const [copied, setCopied] = useState(false);
    const [loading, setLoading] = useState(false);
    const [faucetLoading, setFaucetLoading] = useState(false);

    // Fetch wallet state on mount
    useEffect(() => {
        fetchState();
    }, []);

    async function sendMessage(type: string, payload?: unknown): Promise<any> {
        return chrome.runtime.sendMessage({ type, payload, id: Date.now().toString() });
    }

    async function fetchState() {
        setLoading(true);
        try {
            const response = await sendMessage('GET_STATE');
            setState(response.result as WalletState);

            if (response.result?.address) {
                const balanceResponse = await sendMessage('GET_BALANCE');
                setBalance(balanceResponse.result || 0);
            }
        } catch (error) {
            console.error('Failed to fetch state:', error);
        } finally {
            setLoading(false);
        }
    }

    async function handleCopyAddress() {
        if (state?.address) {
            await navigator.clipboard.writeText(state.address);
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        }
    }

    async function handleFaucet() {
        if (!state?.address) return;

        setFaucetLoading(true);
        try {
            await sendMessage('FAUCET_REQUEST', { address: state.address, amount: 50 });
            // Wait a bit for block to be mined, then refresh balance
            setTimeout(fetchState, 2000);
        } catch (error) {
            console.error('Faucet failed:', error);
        } finally {
            setFaucetLoading(false);
        }
    }

    function formatSats(sats: number): string {
        if (sats >= 100_000_000) {
            return `${(sats / 100_000_000).toFixed(4)} BTC`;
        }
        return `${sats.toLocaleString()} sats`;
    }

    function shortenAddress(address: string): string {
        return `${address.slice(0, 8)}...${address.slice(-6)}`;
    }

    if (loading && !state) {
        return (
            <div className="flex items-center justify-center h-full min-h-[480px] bg-[#0a0a0f]">
                <RefreshCw className="w-8 h-8 text-blue-500 animate-spin" />
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full min-h-[480px] bg-[#0a0a0f] text-white">
            {/* Header */}
            <header className="flex items-center justify-between px-4 py-3 border-b border-[#2a2a3a]">
                <div className="flex items-center gap-2">
                    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-orange-500 to-amber-600 flex items-center justify-center">
                        <Wallet className="w-4 h-4 text-white" />
                    </div>
                    <div>
                        <h1 className="text-sm font-semibold">Isomer Companion</h1>
                        <span className="text-xs text-orange-400 font-medium">regtest</span>
                    </div>
                </div>
                <button className="p-2 hover:bg-[#1a1a24] rounded-lg transition-colors">
                    <Settings className="w-4 h-4 text-gray-400" />
                </button>
            </header>

            {/* Main Content */}
            <main className="flex-1 p-4">
                {state?.address ? (
                    <div className="space-y-4">
                        {/* Address Card */}
                        <div className="bg-[#12121a] border border-[#2a2a3a] rounded-xl p-4">
                            <div className="text-xs text-gray-400 mb-1">Address</div>
                            <div className="flex items-center justify-between">
                                <code className="text-sm font-mono text-gray-200">
                                    {shortenAddress(state.address)}
                                </code>
                                <button
                                    onClick={handleCopyAddress}
                                    className="p-2 hover:bg-[#1a1a24] rounded-lg transition-colors"
                                >
                                    {copied ? (
                                        <Check className="w-4 h-4 text-green-500" />
                                    ) : (
                                        <Copy className="w-4 h-4 text-gray-400" />
                                    )}
                                </button>
                            </div>
                        </div>

                        {/* Balance Card */}
                        <div className="bg-[#12121a] border border-[#2a2a3a] rounded-xl p-4">
                            <div className="flex items-center justify-between mb-2">
                                <span className="text-xs text-gray-400">Balance</span>
                                <button
                                    onClick={fetchState}
                                    className="p-1 hover:bg-[#1a1a24] rounded transition-colors"
                                >
                                    <RefreshCw className={`w-3 h-3 text-gray-400 ${loading ? 'animate-spin' : ''}`} />
                                </button>
                            </div>
                            <div className="text-2xl font-bold text-white">
                                {formatSats(balance)}
                            </div>
                        </div>

                        {/* Faucet Button */}
                        <button
                            onClick={handleFaucet}
                            disabled={faucetLoading}
                            className="w-full flex items-center justify-center gap-2 py-3 px-4 bg-gradient-to-r from-blue-600 to-blue-500 hover:from-blue-500 hover:to-blue-400 disabled:opacity-50 disabled:cursor-not-allowed rounded-xl font-medium transition-all"
                        >
                            {faucetLoading ? (
                                <RefreshCw className="w-4 h-4 animate-spin" />
                            ) : (
                                <Droplets className="w-4 h-4" />
                            )}
                            Request Test Funds
                        </button>

                        {/* Info */}
                        <div className="bg-[#12121a] border border-[#2a2a3a] rounded-xl p-4 space-y-2">
                            <div className="text-xs text-gray-400">Connected to Isomer</div>
                            <div className="flex items-center gap-2">
                                <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
                                <span className="text-sm text-gray-300">localhost:18888</span>
                            </div>
                        </div>

                        {/* dApp Integration Info */}
                        <div className="bg-gradient-to-r from-orange-500/10 to-amber-500/10 border border-orange-500/20 rounded-xl p-4">
                            <div className="text-xs text-orange-400 font-medium mb-1">For dApps</div>
                            <code className="text-xs text-gray-300 break-all">
                                window.alkanes.requestAccounts()
                            </code>
                        </div>
                    </div>
                ) : (
                    <div className="flex flex-col items-center justify-center h-full text-center">
                        <Wallet className="w-12 h-12 text-gray-600 mb-4" />
                        <p className="text-gray-400">Initializing wallet...</p>
                    </div>
                )}
            </main>

            {/* Footer */}
            <footer className="px-4 py-3 border-t border-[#2a2a3a] text-center">
                <a
                    href="https://alkanes.build"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-xs text-gray-500 hover:text-gray-400 flex items-center justify-center gap-1"
                >
                    Powered by Alkanes
                    <ExternalLink className="w-3 h-3" />
                </a>
            </footer>
        </div>
    );
}

export default App;
