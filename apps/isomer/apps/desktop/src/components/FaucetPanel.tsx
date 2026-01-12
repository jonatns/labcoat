import { useState } from 'react';
import { api } from '../lib/api';

interface FaucetPanelProps {
    disabled?: boolean;
}

export function FaucetPanel({ disabled }: FaucetPanelProps) {
    const [address, setAddress] = useState('');
    const [amount, setAmount] = useState('1');
    const [isSending, setIsSending] = useState(false);
    const [result, setResult] = useState<{ success: boolean; message: string } | null>(null);

    const handleFaucet = async () => {
        if (!address.trim()) {
            setResult({ success: false, message: 'Please enter an address' });
            return;
        }

        setIsSending(true);
        setResult(null);

        try {
            const txid = await api.faucet(address.trim(), parseFloat(amount) || 1);
            setResult({ success: true, message: `Sent! TXID: ${txid.slice(0, 16)}...` });
            setAddress('');
        } catch (err) {
            console.error('Faucet error:', err);
            setResult({ success: false, message: err instanceof Error ? err.message : String(err) });
        } finally {
            setIsSending(false);
        }
    };

    return (
        <div className="glass rounded-xl p-5">
            <div className="flex items-center gap-2 mb-4">
                <span className="text-xl">🚰</span>
                <h3 className="text-lg font-semibold text-white">Faucet</h3>
            </div>

            <p className="text-zinc-400 text-sm mb-4">
                Send regtest coins to any address from the dev wallet
            </p>

            <div className="space-y-3">
                <div>
                    <label className="block text-sm text-zinc-500 mb-1">Recipient Address</label>
                    <input
                        type="text"
                        value={address}
                        onChange={(e) => setAddress(e.target.value)}
                        placeholder="bcrt1..."
                        disabled={disabled || isSending}
                        className="w-full px-3 py-2 bg-zinc-800/50 border border-zinc-700 rounded-lg 
                                 text-white placeholder-zinc-500 text-sm font-mono
                                 focus:outline-none focus:border-indigo-500 disabled:opacity-50"
                    />
                </div>

                <div className="flex gap-3">
                    <div className="flex-1">
                        <label className="block text-sm text-zinc-500 mb-1">Amount (BTC)</label>
                        <input
                            type="number"
                            value={amount}
                            onChange={(e) => setAmount(e.target.value)}
                            min="0.001"
                            step="0.1"
                            disabled={disabled || isSending}
                            className="w-full px-3 py-2 bg-zinc-800/50 border border-zinc-700 rounded-lg 
                                     text-white text-sm
                                     focus:outline-none focus:border-indigo-500 disabled:opacity-50"
                        />
                    </div>

                    <div className="flex items-end">
                        <button
                            onClick={handleFaucet}
                            disabled={disabled || isSending || !address.trim()}
                            className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 
                                     disabled:cursor-not-allowed rounded-lg text-white font-medium 
                                     transition-colors whitespace-nowrap"
                        >
                            {isSending ? 'Sending...' : 'Send BTC'}
                        </button>
                    </div>
                </div>

                {result && (
                    <div className={`text-sm px-3 py-2 rounded-lg ${result.success
                            ? 'bg-green-600/20 text-green-400 border border-green-600/50'
                            : 'bg-red-600/20 text-red-400 border border-red-600/50'
                        }`}>
                        {result.message}
                    </div>
                )}
            </div>
        </div>
    );
}
