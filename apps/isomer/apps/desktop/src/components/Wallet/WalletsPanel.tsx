
import { useState, useEffect, useCallback } from 'react';
import { RefreshCw, Wallet, Copy, Check, FolderOpen, Terminal, ArrowRight, WalletCards, ChevronRight, ChevronDown } from 'lucide-react';
import { api } from '../../lib/api';
import type { AlkanesWallet } from '../../lib/types';

interface WalletsPanelProps {
    isRunning: boolean;
}

export function WalletsPanel({ isRunning }: WalletsPanelProps) {
    const [wallets, setWallets] = useState<AlkanesWallet[]>([]);
    const [loading, setLoading] = useState(false);
    const [selectedWalletId, setSelectedWalletId] = useState<string | null>(null);
    const [fundingLoading, setFundingLoading] = useState(false);
    const [copiedAddr, setCopiedAddr] = useState<string | null>(null);
    const [showAddresses, setShowAddresses] = useState(false);

    const activeWallet = wallets.find(w => w.name === selectedWalletId);

    const loadWallets = async () => {
        setLoading(true);
        try {
            const list = await api.getAlkanesWallets();
            setWallets(list);
            if (!selectedWalletId && list.length > 0) {
                setSelectedWalletId(list[0].name);
            }
        } catch (e) {
            console.error("Failed to load wallets:", e);
        } finally {
            setLoading(false);
        }
    };

    const refreshActiveWallet = async () => {
        if (!activeWallet) return;
        try {
            const details = await api.getAlkaneWalletDetails(activeWallet.file_path);
            setWallets(prev => prev.map(w => w.name === details.name ? details : w));
        } catch (e) {
            console.error("Failed to refresh wallet:", e);
        }
    };

    const handleFund = async () => {
        if (!activeWallet || !activeWallet.addresses.length || !isRunning) return;
        setFundingLoading(true);

        // Prioritize P2WPKH (Native Segwit) for funding as it's the standard for payments
        const targetAddr = activeWallet.addresses.find(a => a.type_label.includes('P2WPKH'))?.address
            || activeWallet.addresses[0].address;

        try {
            await api.fundAlkaneWallet(targetAddr, 1.0);
            await new Promise(r => setTimeout(r, 1500));
            await refreshActiveWallet();
        } catch (e) {
            console.error("Failed to fund wallet:", e);
        } finally {
            setFundingLoading(false);
        }
    };

    const copyToClipboard = (text: string) => {
        navigator.clipboard.writeText(text);
        setCopiedAddr(text);
        setTimeout(() => setCopiedAddr(null), 1500);
    };

    const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
        if (wallets.length === 0) return;
        const currentIndex = wallets.findIndex(w => w.name === selectedWalletId);

        if (e.key === 'ArrowDown') {
            e.preventDefault();
            const nextIndex = Math.min(wallets.length - 1, currentIndex + 1);
            setSelectedWalletId(wallets[nextIndex].name);
        } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            const prevIndex = Math.max(0, currentIndex - 1);
            setSelectedWalletId(wallets[prevIndex].name);
        }
    }, [wallets, selectedWalletId]);

    useEffect(() => {
        loadWallets();
    }, []);

    useEffect(() => {
        if (activeWallet && (!activeWallet.balance || activeWallet.addresses.length === 0)) {
            refreshActiveWallet();
        }
        // Reset showAddresses when switching wallets
        setShowAddresses(false);
    }, [selectedWalletId]);

    const getShortPath = (path: string) => {
        return path?.replace(/^\/Users\/[^/]+/, '~') || '';
    };

    if (loading && wallets.length === 0) {
        return (
            <div className="h-full flex items-center justify-center text-zinc-500 text-xs gap-2">
                <RefreshCw className="w-3 h-3 animate-spin" />
                <span>Loading system wallets...</span>
            </div>
        );
    }

    if (wallets.length === 0) {
        return (
            <div className="h-full flex flex-col items-center justify-center text-zinc-500 text-xs gap-3">
                <WalletCards className="w-8 h-8 opacity-20" />
                <div className="text-center">
                    <p>No wallets found in system</p>
                    <p className="text-zinc-600 mt-1 font-mono">~/.alkanes/</p>
                </div>
            </div>
        );
    }

    return (
        <div className="h-full flex overflow-hidden bg-zinc-950" onKeyDown={handleKeyDown} tabIndex={0}>
            {/* LEFT COLUMN: Master List */}
            <div className="w-64 flex flex-col border-r border-zinc-900 bg-zinc-950">
                <div className="px-4 py-3 flex items-center justify-between border-b border-zinc-900">
                    <span className="text-[10px] uppercase tracking-wider text-zinc-500 font-bold">Explorer</span>
                    <button
                        onClick={loadWallets}
                        className="text-zinc-600 hover:text-zinc-300 transition-colors p-1 rounded hover:bg-zinc-900"
                    >
                        <RefreshCw className={`w-3 h-3 ${loading ? 'animate-spin' : ''}`} />
                    </button>
                </div>
                <div className="flex-1 overflow-y-auto py-2">
                    {wallets.map(wallet => (
                        <button
                            key={wallet.name}
                            onClick={() => setSelectedWalletId(wallet.name)}
                            className={`
                                w-full text-left px-4 py-3 flex items-center gap-3 transition-all border-l-2
                                ${selectedWalletId === wallet.name
                                    ? 'bg-zinc-900 border-indigo-500 text-zinc-100'
                                    : 'border-transparent text-zinc-400 hover:text-zinc-200 hover:bg-zinc-900/50'}
                            `}
                        >
                            <Wallet className={`w-4 h-4 flex-none ${selectedWalletId === wallet.name ? 'text-indigo-400' : 'text-zinc-600'}`} />
                            <div className="flex-1 min-w-0">
                                <div className="text-xs font-medium truncate">{wallet.name}</div>
                                {wallet.balance && (
                                    <div className="text-[10px] font-mono text-zinc-500 truncate mt-0.5">
                                        {wallet.balance.split('\n')[0]}
                                    </div>
                                )}
                            </div>
                        </button>
                    ))}
                </div>
            </div>

            {/* RIGHT COLUMN: Inspector Detail */}
            <div className="flex-1 flex flex-col bg-zinc-950">
                {activeWallet ? (
                    <div className="flex-1 flex flex-col overflow-hidden">
                        {/* Header: Identity & Context */}
                        <div className="px-8 py-6 border-b border-zinc-900 flex items-start justify-between bg-zinc-950">
                            <div>
                                <div className="flex items-center gap-3 mb-1">
                                    <h2 className="text-2xl font-bold text-white tracking-tight">
                                        {activeWallet.name}
                                    </h2>
                                    <span className="px-2 py-0.5 rounded text-[10px] bg-amber-500/10 text-amber-500 font-bold uppercase tracking-wider border border-amber-500/20">
                                        Regtest
                                    </span>
                                </div>
                                <div className="flex items-center gap-2 text-zinc-500 text-xs font-mono group">
                                    <FolderOpen className="w-3.5 h-3.5" />
                                    <span className="truncate max-w-md select-all">{getShortPath(activeWallet.file_path)}</span>
                                    <button
                                        onClick={() => copyToClipboard(activeWallet.file_path)}
                                        className="opacity-0 group-hover:opacity-100 hover:text-zinc-300 transition-opacity p-1"
                                    >
                                        <Copy className="w-3 h-3" />
                                    </button>
                                </div>
                            </div>
                        </div>

                        <div className="flex-1 overflow-y-auto">
                            <div className="px-8 py-8 space-y-10 max-w-3xl">

                                {/* Section 1: Balance (Hero) */}
                                <section>
                                    <div className="text-sm uppercase tracking-wider text-zinc-500 font-medium mb-2">Total Balance</div>
                                    <div className="flex items-baseline gap-3">
                                        <span className="text-4xl font-mono font-light text-white tracking-tight">
                                            {activeWallet.balance ? activeWallet.balance.split(' ')[0] : "0.00000000"}
                                        </span>
                                        <span className="text-xl text-zinc-500 font-medium">BTC</span>
                                    </div>
                                </section>

                                {/* Section 2: Primary Actions */}
                                <section>
                                    <div className="text-[10px] uppercase tracking-wider text-zinc-500 font-bold mb-4 flex items-center gap-2">
                                        <Terminal className="w-3 h-3" />
                                        Actions
                                    </div>
                                    <div className="flex items-center gap-3">
                                        <button
                                            onClick={handleFund}
                                            disabled={fundingLoading || !isRunning}
                                            className="
                                                px-5 py-2.5 rounded-md bg-zinc-100 text-zinc-950 
                                                text-sm font-semibold shadow-lg shadow-indigo-500/10
                                                hover:bg-white hover:scale-[1.02] active:scale-[0.98]
                                                disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100
                                                flex items-center gap-2 transition-all duration-200
                                            "
                                        >
                                            {fundingLoading ? (
                                                <RefreshCw className="w-4 h-4 animate-spin text-indigo-600" />
                                            ) : (
                                                <div className="w-2 h-2 rounded-full bg-indigo-500" />
                                            )}
                                            Fund Wallet
                                        </button>
                                    </div>
                                    <p className="mt-3 text-[10px] text-zinc-600 max-w-xs">
                                        Funds 1.0 BTC. Prioritizes Native Segwit (P2WPKH) if available.
                                    </p>
                                </section>

                                {/* Section 3: Addresses (Tools) */}
                                <section className="pt-2">
                                    <button
                                        onClick={() => setShowAddresses(!showAddresses)}
                                        className="w-full flex items-center justify-between group py-2 border-b border-zinc-900 hover:border-zinc-700 transition-colors"
                                    >
                                        <div className="flex items-center gap-2 text-[10px] uppercase tracking-wider text-zinc-500 font-bold group-hover:text-zinc-300">
                                            <ArrowRight className={`w-3 h-3 transition-transform duration-200 ${showAddresses ? 'rotate-90' : ''}`} />
                                            Addresses ({activeWallet.addresses.length})
                                        </div>
                                        <div className="text-[10px] text-zinc-600 group-hover:text-zinc-400">
                                            {showAddresses ? 'Hide Details' : 'Show Details'}
                                        </div>
                                    </button>

                                    {showAddresses && (
                                        <div className="mt-4 space-y-1 animate-in slide-in-from-top-2 fade-in duration-200">
                                            {activeWallet.addresses.length > 0 ? (
                                                activeWallet.addresses.map((addrInfo, i) => (
                                                    <div
                                                        key={`${addrInfo.address}-${i}`}
                                                        className="
                                                            flex items-center justify-between px-3 py-2 
                                                            rounded border border-transparent
                                                            hover:bg-zinc-900 hover:border-zinc-800 
                                                            group transition-all cursor-default
                                                        "
                                                    >
                                                        <div className="flex items-center gap-4 overflow-hidden">
                                                            <div className="flex flex-col items-end min-w-[3rem]">
                                                                <span className="text-[10px] font-bold text-zinc-500 uppercase tracking-tight">
                                                                    {addrInfo.type_label}
                                                                </span>
                                                                <span className="text-[9px] font-mono text-zinc-600">
                                                                    #{addrInfo.index}
                                                                </span>
                                                            </div>
                                                            <code className="text-xs text-zinc-400 font-mono truncate select-all group-hover:text-zinc-200">
                                                                {addrInfo.address}
                                                            </code>
                                                        </div>
                                                        <button
                                                            onClick={() => copyToClipboard(addrInfo.address)}
                                                            className="
                                                                p-1.5 rounded text-zinc-600 
                                                                hover:text-zinc-100 hover:bg-zinc-800 
                                                                opacity-0 group-hover:opacity-100 transition-all
                                                            "
                                                            title="Copy Address"
                                                        >
                                                            {copiedAddr === addrInfo.address ? (
                                                                <Check className="w-3.5 h-3.5 text-emerald-500" />
                                                            ) : (
                                                                <Copy className="w-3.5 h-3.5" />
                                                            )}
                                                        </button>
                                                    </div>
                                                ))
                                            ) : (
                                                <div className="text-zinc-600 text-xs italic py-2">
                                                    No addresses found. activeWallet
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </section>
                            </div>
                        </div>
                    </div>
                ) : (
                    <div className="flex-1 flex flex-col items-center justify-center text-zinc-600 gap-4">
                        <Wallet className="w-12 h-12 opacity-20" />
                        <span className="text-sm">Select a wallet to inspect</span>
                    </div>
                )}
            </div>
        </div>
    );
}
