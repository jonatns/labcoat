/**
 * ContractsPanel - Display deployed Alkane contracts from Espo
 */

import { useState, useEffect, useCallback } from "react";
import {
    Package,
    RefreshCw,
    ExternalLink,
    Clock,
    Users,
    Hash,
} from "lucide-react";
import { api } from "../lib/api";
import type { AlkaneInfo } from "../lib/types";
import { espo } from "../lib/rpc/espoClient";

export function ContractsPanel() {
    const [contracts, setContracts] = useState<AlkaneInfo[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [page, setPage] = useState(1);
    const [total, setTotal] = useState(0);
    const limit = 20;

    const fetchContracts = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const response = await api.getAllAlkanes(page, limit);
            if (response.ok) {
                setContracts(response.items);
                setTotal(response.total);
            } else {
                setError("Failed to fetch contracts");
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : "Unknown error");
        } finally {
            setLoading(false);
        }
    }, [page]);

    useEffect(() => {
        fetchContracts();
    }, [fetchContracts]);

    const formatTimestamp = (ts: number | null) => {
        if (!ts) return "Unknown";
        return new Date(ts * 1000).toLocaleString();
    };

    const truncateTxid = (txid: string) => {
        return `${txid.slice(0, 8)}...${txid.slice(-8)}`;
    };

    const hasMore = page * limit < total;
    const hasPrev = page > 1;

    return (
        <div className="flex flex-col h-full bg-zinc-900 text-white">
            {/* Header */}
            <div className="flex items-center justify-between p-4 border-b border-zinc-700">
                <div className="flex items-center gap-2">
                    <Package className="w-5 h-5 text-blue-400" />
                    <h2 className="text-lg font-semibold">Deployed Contracts</h2>
                    <span className="px-2 py-0.5 text-xs bg-zinc-700 rounded-full">
                        {total}
                    </span>
                </div>
                <button
                    onClick={fetchContracts}
                    disabled={loading}
                    className="p-2 rounded-lg bg-zinc-800 hover:bg-zinc-700 transition-colors disabled:opacity-50"
                    title="Refresh"
                >
                    <RefreshCw className={`w-4 h-4 ${loading ? "animate-spin" : ""}`} />
                </button>
            </div>

            {/* Content */}
            <div className="flex-1 overflow-y-auto p-4">
                {loading && contracts.length === 0 ? (
                    <div className="flex items-center justify-center h-full text-zinc-500">
                        <RefreshCw className="w-6 h-6 animate-spin mr-2" />
                        Loading contracts...
                    </div>
                ) : error ? (
                    <div className="flex flex-col items-center justify-center h-full text-zinc-500">
                        <Package className="w-12 h-12 mb-2 opacity-50" />
                        <p className="text-red-400">{error}</p>
                        <button
                            onClick={fetchContracts}
                            className="mt-4 px-4 py-2 rounded-lg bg-blue-600 hover:bg-blue-500 transition-colors"
                        >
                            Retry
                        </button>
                    </div>
                ) : contracts.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-full text-zinc-500">
                        <Package className="w-12 h-12 mb-2 opacity-50" />
                        <p>No contracts deployed yet</p>
                        <p className="text-sm mt-1">
                            Deploy contracts and they will appear here
                        </p>
                    </div>
                ) : (
                    <div className="grid gap-3">
                        {contracts.map((contract) => (
                            <div
                                key={contract.alkane}
                                className="p-4 rounded-lg bg-zinc-800 border border-zinc-700 hover:border-zinc-600 transition-colors"
                            >
                                {/* Contract Header */}
                                <div className="flex items-center justify-between mb-3">
                                    <div className="flex items-center gap-2">
                                        <span className="font-mono text-blue-400 font-medium">
                                            {contract.name || contract.alkane}
                                        </span>
                                        {contract.symbol && (
                                            <span className="px-2 py-0.5 text-xs bg-zinc-700 rounded">
                                                {contract.symbol}
                                            </span>
                                        )}
                                    </div>
                                    <a
                                        href={espo.getAlkaneUrl(contract.alkane)}
                                        target="_blank"
                                        rel="noopener noreferrer"
                                        className="p-1.5 rounded bg-zinc-700 hover:bg-zinc-600 transition-colors"
                                        title="View in Espo"
                                    >
                                        <ExternalLink className="w-3.5 h-3.5" />
                                    </a>
                                </div>

                                {/* Contract Details */}
                                <div className="grid grid-cols-2 gap-2 text-sm text-zinc-400">
                                    <div className="flex items-center gap-1.5">
                                        <Hash className="w-3.5 h-3.5" />
                                        <span className="font-mono">{contract.alkane}</span>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <Users className="w-3.5 h-3.5" />
                                        <span>{contract.holder_count} holders</span>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <Clock className="w-3.5 h-3.5" />
                                        <span>
                                            Block {contract.creation_height}
                                        </span>
                                    </div>
                                    <div
                                        className="flex items-center gap-1.5 font-mono text-xs"
                                        title={contract.creation_txid}
                                    >
                                        <span className="text-zinc-500">txid:</span>
                                        {truncateTxid(contract.creation_txid)}
                                    </div>
                                </div>
                            </div>
                        ))}
                    </div>
                )}
            </div>

            {/* Pagination */}
            {total > limit && (
                <div className="flex items-center justify-between p-4 border-t border-zinc-700">
                    <button
                        onClick={() => setPage((p) => Math.max(1, p - 1))}
                        disabled={!hasPrev || loading}
                        className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        Previous
                    </button>
                    <span className="text-sm text-zinc-400">
                        Page {page} of {Math.ceil(total / limit)}
                    </span>
                    <button
                        onClick={() => setPage((p) => p + 1)}
                        disabled={!hasMore || loading}
                        className="px-3 py-1.5 rounded bg-zinc-800 hover:bg-zinc-700 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        Next
                    </button>
                </div>
            )}
        </div>
    );
}
