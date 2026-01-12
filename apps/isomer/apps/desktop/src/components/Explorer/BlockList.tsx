import { CarouselBlock, espo } from '../../lib/rpc/espoClient';

function timeAgo(timestamp: number | null): string {
    if (timestamp === null) return '—';
    const seconds = Math.floor((Date.now() / 1000) - timestamp);

    if (seconds < 60) return `${seconds}s ago`;
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
}

interface BlockListProps {
    blocks: CarouselBlock[];
    isLoading: boolean;
}

export function BlockList({ blocks, isLoading }: BlockListProps) {
    if (isLoading && blocks.length === 0) {
        return (
            <div className="w-full h-48 flex items-center justify-center text-zinc-500 animate-pulse text-sm">
                Loading blocks...
            </div>
        );
    }

    if (blocks.length === 0) {
        return (
            <div className="w-full h-24 flex items-center justify-center text-zinc-500 text-sm">
                No blocks found.
            </div>
        );
    }

    const openBlock = (height: number) => {
        window.open(espo.getBlockUrl(height), '_blank');
    };

    return (
        <div className="w-full overflow-hidden rounded-lg border border-zinc-800 bg-zinc-900/30">
            <table className="w-full text-left text-xs">
                <thead className="bg-zinc-900/50 text-zinc-500 font-medium uppercase tracking-wider">
                    <tr>
                        <th className="px-3 py-2.5 w-24">Height</th>
                        <th className="px-3 py-2.5 w-20 text-right">Traces</th>
                        <th className="px-3 py-2.5 text-right">Time</th>
                    </tr>
                </thead>
                <tbody className="divide-y divide-zinc-800/50">
                    {blocks.map((block) => (
                        <tr
                            key={block.height}
                            onClick={() => openBlock(block.height)}
                            className="hover:bg-zinc-800/30 transition-colors cursor-pointer group"
                        >
                            <td className="px-3 py-2 text-indigo-400 font-mono group-hover:text-indigo-300">
                                #{block.height.toLocaleString()}
                            </td>
                            <td className="px-3 py-2 text-right">
                                {block.traces > 0 ? (
                                    <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium bg-amber-500/10 text-amber-400 border border-amber-500/20">
                                        {block.traces}
                                    </span>
                                ) : (
                                    <span className="text-zinc-600">0</span>
                                )}
                            </td>
                            <td className="px-3 py-2 text-right text-zinc-500 font-mono">
                                {timeAgo(block.time)}
                            </td>
                        </tr>
                    ))}
                </tbody>
            </table>
        </div>
    );
}
