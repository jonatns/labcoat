import { useEffect, useState, useCallback } from 'react';
import { espo, CarouselBlock } from '../../lib/rpc/espoClient';
import { BlockList } from './BlockList';
import { RefreshCw, AlertCircle, ExternalLink } from 'lucide-react';

export function ExplorerHome() {
    const [blocks, setBlocks] = useState<CarouselBlock[]>([]);
    const [espoTip, setEspoTip] = useState<number>(0);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);
    const [lastUpdated, setLastUpdated] = useState<Date>(new Date());

    const fetchBlocks = useCallback(async () => {
        try {
            setError(null);
            const response = await espo.getCarouselBlocks(undefined, 15);
            // Reverse to show newest first
            setBlocks(response.blocks.reverse());
            setEspoTip(response.espo_tip);
            setLastUpdated(new Date());
        } catch (err) {
            console.error('Failed to fetch blocks:', err);
            setError('Failed to connect to Espo. Is the explorer running?');
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        fetchBlocks();
        // Poll every 3 seconds
        const interval = setInterval(fetchBlocks, 3000);
        return () => clearInterval(interval);
    }, [fetchBlocks]);

    return (
        <div className="h-full flex flex-col space-y-4 p-4 overflow-y-auto">
            {/* Header Stats */}
            <div className="flex items-center justify-between">
                <div>
                    <h2 className="text-lg font-semibold text-zinc-100">Latest Blocks</h2>
                    <p className="text-xs text-zinc-500">
                        Tip: #{espoTip.toLocaleString()} · Updated {lastUpdated.toLocaleTimeString()}
                    </p>
                </div>

                <button
                    onClick={fetchBlocks}
                    className="p-2 hover:bg-zinc-900 rounded-md text-zinc-400 hover:text-white transition-colors"
                    title="Refresh"
                >
                    <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
                </button>
            </div>

            {error && (
                <div className="bg-red-500/10 border border-red-500/20 text-red-400 px-3 py-2 rounded-md flex items-center gap-2 text-xs">
                    <AlertCircle size={14} />
                    {error}
                </div>
            )}

            {/* Main Content */}
            <BlockList blocks={blocks} isLoading={isLoading && blocks.length === 0} />

            {/* Footer / Connection Info */}
            <div className="text-xs text-zinc-600 flex items-center justify-between mt-auto pt-3 border-t border-zinc-900/50">
                <div className="flex items-center gap-2">
                    <div className="w-1.5 h-1.5 rounded-full bg-emerald-500/50 animate-pulse" />
                    Connected to Espo Explorer
                </div>
                <a
                    href="http://localhost:8081"
                    target="_blank"
                    rel="noopener noreferrer"
                    className="flex items-center gap-1 text-zinc-500 hover:text-zinc-300 transition-colors"
                >
                    Full Explorer <ExternalLink size={10} />
                </a>
            </div>
        </div>
    );
}
