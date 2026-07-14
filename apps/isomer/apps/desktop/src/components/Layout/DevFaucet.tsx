import { useState } from 'react';
import { Zap, Check, Wallet, Loader2, ArrowRight } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface DevFaucetProps {
    isRunning: boolean;
}

export function DevFaucet({ isRunning }: DevFaucetProps) {
    const [address, setAddress] = useState('');
    const [status, setStatus] = useState<'idle' | 'sending' | 'success' | 'error'>('idle');
    const [errorMsg, setErrorMsg] = useState('');

    const handleTopUp = async (e: React.FormEvent) => {
        e.preventDefault();
        if (!address.trim() || status === 'sending') return;

        setStatus('sending');
        setErrorMsg('');

        try {
            await invoke('faucet', {
                address: address.trim(),
                amount: 1.0
            });
            setStatus('success');
            setTimeout(() => setStatus('idle'), 3000);
            setAddress('');
        } catch (e: any) {
            console.error('Faucet failed:', e);
            setStatus('error');
            setErrorMsg(e.toString());
            setTimeout(() => setStatus('idle'), 5000);
        }
    };

    if (!isRunning) return null;

    return (
        <div className="w-full max-w-md mx-auto mt-20">
            <form onSubmit={handleTopUp} className="relative group">
                <div className={`
                    absolute -inset-1 rounded-full bg-gradient-to-r from-yellow-500/5 to-amber-500/5 opacity-0 
                    transition duration-500 group-hover:opacity-100 blur-xl
                    ${status === 'error' ? 'from-red-500/5 to-rose-500/5' : ''}
                    ${status === 'success' ? 'from-emerald-500/5 to-green-500/5' : ''}
                `} />

                <div className="relative flex items-center p-1 rounded-full bg-zinc-900 border border-zinc-800 focus-within:border-zinc-700 transition-colors">
                    {/* Icon / Status */}
                    <div className="flex-shrink-0 w-10 h-10 rounded-full bg-zinc-800/50 flex items-center justify-center ml-1">
                        {status === 'sending' ? (
                            <Loader2 className="w-4 h-4 text-yellow-500 animate-spin" />
                        ) : status === 'success' ? (
                            <Check className="w-4 h-4 text-emerald-500" />
                        ) : status === 'error' ? (
                            <Zap className="w-4 h-4 text-red-500" />
                        ) : (
                            <Wallet className="w-4 h-4 text-zinc-500 group-focus-within:text-yellow-500 transition-colors" />
                        )}
                    </div>

                    {/* Input */}
                    <input
                        type="text"
                        value={address}
                        onChange={(e) => setAddress(e.target.value)}
                        placeholder={status === 'error' ? errorMsg : "Enter regtest address for 1 BTC"}
                        disabled={status === 'sending'}
                        className={`
                            flex-1 bg-transparent border-none outline-none px-4 text-sm font-mono
                            placeholder:text-zinc-600 disabled:opacity-50
                            ${status === 'error' ? 'text-red-400 placeholder:text-red-600/50' : 'text-zinc-200'}
                        `}
                    />

                    {/* Action Button */}
                    <button
                        type="submit"
                        disabled={!address.trim() || status === 'sending'}
                        className={`
                            flex items-center gap-2 px-4 py-2 rounded-full text-xs font-bold uppercase tracking-wide transition-all duration-200
                            ${!address.trim()
                                ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
                                : status === 'success'
                                    ? 'bg-emerald-500 text-black'
                                    : 'bg-yellow-500 hover:bg-yellow-400 text-black shadow-[0_0_15px_-3px_rgba(234,179,8,0.4)]'}
                        `}
                    >
                        {status === 'success' ? 'Sent' : 'Top Up'}
                        {!status && <ArrowRight className="w-3 h-3" />}
                    </button>
                </div>
            </form>

            {/* Helper Text */}
            <div className={`
                text-center mt-3 transition-all duration-300 overflow-hidden
                ${address ? 'h-0 opacity-0' : 'h-5 opacity-100'}
            `}>
                <p className="text-[10px] text-zinc-600 font-mono">
                    Target any address • 1 BTC per request • Requires block confirmation
                </p>
            </div>
        </div>
    );
}
