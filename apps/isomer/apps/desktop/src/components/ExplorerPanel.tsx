import { useState, useEffect, useCallback, useRef } from 'react';
import { ExternalLink, RefreshCw, Copy, Check, Loader, Zap, ArrowLeftToLine } from 'lucide-react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { espo, CarouselBlock, BlockDetails } from '../lib/rpc/espoClient';

interface BlockCardProps {
    block: CarouselBlock;
    isTip: boolean;
    isSelected: boolean;
    onClick: () => void;
}

function BlockCard({ block, isTip, isSelected, onClick }: BlockCardProps) {
    const hasTraces = block.traces > 0;
    const label = `Block ${block.height}${isTip ? ', chain tip' : ''}${hasTraces ? `, ${block.traces} trace${block.traces !== 1 ? 's' : ''}` : ''}`;

    return (
        <button
            onClick={onClick}
            role="option"
            aria-selected={isSelected}
            aria-label={label}
            className={`
                flex-shrink-0 w-24 h-28 rounded-lg border flex flex-col items-center justify-center gap-2 transition-all duration-300 relative group
                ${isSelected
                    ? 'bg-amber-500/20 border-amber-500/50 shadow-[0_0_12px_rgba(245,158,11,0.25)]'
                    : hasTraces
                        ? 'bg-amber-500/5 border-amber-500/30 hover:bg-amber-500/10 shadow-[0_0_8px_rgba(245,158,11,0.1)]'
                        : 'bg-zinc-900/50 border-zinc-800 hover:border-zinc-700 hover:bg-zinc-800/80'}
            `}
        >
            <div className={`text-xs font-mono font-medium ${hasTraces ? 'text-amber-400' : 'text-zinc-400'}`}>
                #{block.height}
            </div>

            {hasTraces && (
                <div className="text-[10px] font-bold text-amber-500 bg-amber-500/10 px-1.5 py-0.5 rounded">
                    {block.traces} trace{block.traces !== 1 ? 's' : ''}
                </div>
            )}

            <div className="text-[9px] text-zinc-600 font-mono">
                {block.time ? new Date(block.time * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '—'}
            </div>

            {isTip && (
                <div className="absolute -top-2 left-1/2 -translate-x-1/2 px-1.5 py-0.5 bg-emerald-500 text-[9px] font-bold text-emerald-950 rounded uppercase tracking-tight shadow-lg">
                    Tip
                </div>
            )}

            {isSelected && (
                <div className="absolute -bottom-0.5 left-1/2 -translate-x-1/2 w-1 h-1 bg-amber-500 rounded-full" />
            )}
        </button>
    );
}

interface BlockDetailProps {
    block: CarouselBlock;
    details: BlockDetails | null;
    isTip: boolean;
    onViewInEspo: () => void;
}

function BlockDetail({ block, details, isTip, onViewInEspo }: BlockDetailProps) {
    const [copied, setCopied] = useState(false);
    const [copiedTx, setCopiedTx] = useState<string | null>(null);

    const handleCopy = () => {
        navigator.clipboard.writeText(block.height.toString());
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
    };

    const handleCopyTx = (e: React.MouseEvent, txid: string) => {
        e.stopPropagation();
        navigator.clipboard.writeText(txid);
        setCopiedTx(txid);
        setTimeout(() => setCopiedTx(null), 1500);
    };

    return (
        <div className="relative animate-slide-up">
            {/* Connector caret - only if we want to show it pointing to the card, but since it's a separate section now, maybe skip or adjust */}
            <div className="bg-zinc-900/40 border-t border-zinc-800 backdrop-blur-md overflow-hidden rounded-xl mx-4 mb-4 border-x border-b">
                <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-800/50 bg-zinc-900/50">
                    <div className="flex items-center gap-6">
                        <div>
                            <div className="text-[10px] text-zinc-500 uppercase tracking-wide">Block</div>
                            <div className="text-sm font-mono text-zinc-200 flex items-center gap-2">
                                #{block.height.toLocaleString()}
                                {isTip && <span className="text-[9px] font-sans font-medium text-emerald-500 bg-emerald-500/10 px-1.5 py-0.5 rounded">LIVE</span>}
                                <button
                                    onClick={handleCopy}
                                    className="p-1 hover:bg-zinc-800 rounded text-zinc-500 hover:text-zinc-300 transition-colors"
                                    title="Copy Height"
                                >
                                    {copied ? <Check size={11} className="text-green-400" /> : <Copy size={11} />}
                                </button>
                            </div>
                        </div>
                        <div>
                            <div className="text-[10px] text-zinc-500 uppercase tracking-wide flex items-center gap-1">
                                <Zap size={10} className={block.traces > 0 ? 'text-amber-500' : 'text-zinc-600'} />
                                Traces
                            </div>
                            <div className={`text-sm font-medium ${block.traces > 0 ? 'text-amber-400' : 'text-zinc-400'}`}>
                                {block.traces}
                            </div>
                        </div>
                        <div>
                            <div className="text-[10px] text-zinc-500 uppercase tracking-wide">Time</div>
                            <div className="text-sm text-zinc-400 font-mono">
                                {block.time ? new Date(block.time * 1000).toLocaleTimeString() : '—'}
                            </div>
                        </div>
                    </div>
                    <button
                        onClick={onViewInEspo}
                        className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-indigo-400 hover:text-indigo-300 bg-indigo-500/10 hover:bg-indigo-500/20 border border-indigo-500/30 rounded-md transition-colors"
                    >
                        View in Espo
                        <ExternalLink size={10} />
                    </button>
                </div>

                {/* Transactions Section */}
                <div className="px-4 py-3 bg-zinc-950/40">
                    <div className="flex items-center justify-between mb-2">
                        <div className="text-[10px] text-zinc-500 uppercase tracking-wide flex items-center gap-2">
                            Transactions
                            {details ? (
                                <span className="text-zinc-400 bg-zinc-800 px-1.5 py-0.5 rounded text-[9px]">{details.transactions.length}</span>
                            ) : (
                                <Loader size={10} className="animate-spin text-amber-500" />
                            )}
                        </div>
                        {details && (
                            <div className="text-[9px] font-mono text-zinc-600 truncate max-w-[200px]" title={details.hash}>
                                Hash: {details.hash.slice(0, 16)}...
                            </div>
                        )}
                    </div>

                    <div className="flex flex-col gap-1.5 max-h-56 overflow-y-auto pr-2 custom-scrollbar min-h-[40px]">
                        {details?.transactions.length === 0 ? (
                            <div className="h-12 flex items-center justify-center text-zinc-600 italic text-[10px]">
                                No transactions in this block.
                            </div>
                        ) : details?.transactions ? (
                            details.transactions.map((txInfo, index) => {
                                const isCoinbase = index === 0;
                                const isTrace = txInfo.is_trace;
                                const txid = txInfo.txid;

                                return (
                                    <div
                                        key={txid}
                                        className={`
                                            flex items-center gap-2 p-2 rounded-md transition-all cursor-pointer group
                                            ${isTrace
                                                ? 'bg-amber-500/5 border border-amber-500/20 hover:bg-amber-500/10 hover:border-amber-500/40'
                                                : 'bg-zinc-900/50 border border-zinc-800/50 hover:bg-zinc-800 hover:border-zinc-700'}
                                        `}
                                        onClick={() => openUrl(espo.getTxUrl(txid))}
                                    >
                                        {/* Trace Badge */}
                                        <div className="flex-shrink-0 w-14 flex justify-center">
                                            {isTrace ? (
                                                <span className="px-1.5 py-0.5 text-[8px] font-bold bg-amber-500/15 text-amber-400 rounded border border-amber-500/30 flex items-center gap-0.5">
                                                    <Zap size={8} />
                                                    TRACE
                                                </span>
                                            ) : isCoinbase ? (
                                                <span className="text-[8px] text-zinc-600 uppercase">Coinbase</span>
                                            ) : (
                                                <span className="text-[8px] text-zinc-700">—</span>
                                            )}
                                        </div>

                                        {/* TXID */}
                                        <div className={`flex-1 font-mono text-[10px] truncate ${isTrace ? 'text-amber-300' : 'text-zinc-400'}`}>
                                            {txid.slice(0, 12)}...{txid.slice(-12)}
                                        </div>

                                        {/* Actions */}
                                        <div className="flex items-center gap-1 flex-shrink-0">
                                            <button
                                                onClick={(e) => handleCopyTx(e, txid)}
                                                className="p-1 text-zinc-600 hover:text-zinc-300 hover:bg-zinc-700/50 rounded transition-colors"
                                                title="Copy TXID"
                                            >
                                                {copiedTx === txid ? <Check size={10} className="text-green-500" /> : <Copy size={10} />}
                                            </button>
                                            <ExternalLink size={10} className="text-zinc-700 group-hover:text-zinc-400" />
                                        </div>
                                    </div>
                                );
                            })
                        ) : null}
                    </div>
                </div>
            </div>
        </div>
    );
}

interface ExplorerPanelProps {
    onBlockSelect?: () => void;
    isVisible?: boolean;
    isMaximized?: boolean;
}

export function ExplorerPanel({ onBlockSelect, isVisible = true, isMaximized = false }: ExplorerPanelProps) {
    const [blocks, setBlocks] = useState<CarouselBlock[]>([]);
    const [espoTip, setEspoTip] = useState<number>(0);
    const [isLoading, setIsLoading] = useState(true);
    const [isLoadingMore, setIsLoadingMore] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [selectedBlock, setSelectedBlock] = useState<CarouselBlock | null>(null);
    const [selectedBlockDetails, setSelectedBlockDetails] = useState<BlockDetails | null>(null);
    const [isNewBlock, setIsNewBlock] = useState(false);
    const [hasMoreBlocks, setHasMoreBlocks] = useState(true);
    const [showScrollLatest, setShowScrollLatest] = useState(false);
    const [focusedIndex, setFocusedIndex] = useState<number>(-1);

    const scrollContainerRef = useRef<HTMLDivElement>(null);
    const sentinelRef = useRef<HTMLDivElement>(null);
    const currentTipRef = useRef<number>(0);

    const scrollToLatest = () => {
        scrollContainerRef.current?.scrollTo({ left: 0, behavior: 'smooth' });
    };

    // Track previous visibility to detect close transitions
    const prevVisibleRef = useRef(isVisible);

    // Reset selection state when panel closes (transitions from visible to hidden)
    useEffect(() => {
        if (prevVisibleRef.current && !isVisible) {
            // Panel just closed - reset selection
            setSelectedBlock(null);
            setSelectedBlockDetails(null);
            setFocusedIndex(-1);
        }
        prevVisibleRef.current = isVisible;
    }, [isVisible]);

    // Track previous maximize state to detect un-maximize transitions
    const prevMaximizedRef = useRef(isMaximized);

    // Reset selection state when un-maximizing (maximized -> normal expanded)
    useEffect(() => {
        if (prevMaximizedRef.current && !isMaximized) {
            // Panel just un-maximized - reset selection
            setSelectedBlock(null);
            setSelectedBlockDetails(null);
            setFocusedIndex(-1);
        }
        prevMaximizedRef.current = isMaximized;
    }, [isMaximized]);

    // Track scroll position to show/hide "Jump to Latest" button
    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) return;

        const handleScroll = () => {
            setShowScrollLatest(container.scrollLeft > 200);
        };

        container.addEventListener('scroll', handleScroll, { passive: true });
        return () => container.removeEventListener('scroll', handleScroll);
    }, []);

    // Initial load - fetch blocks from tip
    const initialFetch = useCallback(async () => {
        try {
            setError(null);
            const response = await espo.getCarouselBlocks(undefined, 30);
            const newBlocks = response.blocks.reverse(); // Newest first

            setBlocks(newBlocks);
            setEspoTip(response.espo_tip);
            currentTipRef.current = response.espo_tip;
            setHasMoreBlocks(newBlocks.length > 0 && newBlocks[newBlocks.length - 1].height > 1);
        } catch (err) {
            console.error('Failed to fetch blocks:', err);
            setError('Failed to connect to Espo');
        } finally {
            setIsLoading(false);
        }
    }, []);

    // Poll for new blocks at tip only - with hydration logic
    const pollForNewBlocks = useCallback(async () => {
        if (currentTipRef.current === 0) return;

        try {
            const response = await espo.getCarouselBlocks(undefined, 10);
            const newTip = response.espo_tip;

            setBlocks(prev => {
                let changed = false;
                const next = prev.map(block => {
                    const match = response.blocks.find(b => b.height === block.height);
                    // Hydrate traces if they changed from 0 to something else
                    if (match && match.traces !== block.traces) {
                        changed = true;
                        return { ...block, traces: match.traces };
                    }
                    return block;
                });

                if (newTip > currentTipRef.current) {
                    const newBlocks = response.blocks
                        .filter(b => b.height > currentTipRef.current)
                        .sort((a, b) => b.height - a.height);

                    if (newBlocks.length > 0) {
                        setIsNewBlock(true);
                        setTimeout(() => setIsNewBlock(false), 1000);
                        changed = true;

                        // Prepends new blocks
                        const combined = [...newBlocks, ...next];
                        setEspoTip(newTip);
                        currentTipRef.current = newTip;
                        return combined;
                    }
                }

                return changed ? next : prev;
            });
        } catch (err) {
            console.error('Poll failed:', err);
        }
    }, []);

    // Optimistically fetch latest block from Bitcoin Core
    const fetchLatestBlock = useCallback(async () => {
        try {
            const latestBlock = await espo.getLatestBlock();

            if (latestBlock.height > currentTipRef.current) {
                // Check if we already have this block (avoid dupes)
                setBlocks(prev => {
                    const exists = prev.some(b => b.height === latestBlock.height);
                    if (exists) return prev;

                    setIsNewBlock(true);
                    setTimeout(() => setIsNewBlock(false), 1000);

                    // Add to top list
                    return [latestBlock, ...prev];
                });

                // Update refs
                setEspoTip(latestBlock.height);
                currentTipRef.current = latestBlock.height;
            }
        } catch (err) {
            console.error('Failed to fetch latest block:', err);
        }
    }, []);

    // Fetch more historical blocks (infinite scroll)
    const fetchMoreBlocks = useCallback(async () => {
        if (isLoadingMore || !hasMoreBlocks || blocks.length === 0) return;

        const oldestHeight = blocks[blocks.length - 1].height;
        if (oldestHeight <= 1) {
            setHasMoreBlocks(false);
            return;
        }

        setIsLoadingMore(true);
        try {
            const centerHeight = Math.max(oldestHeight - 15, 15);
            const response = await espo.getCarouselBlocks(centerHeight, 20);

            // Filter to only blocks older than our current oldest
            const olderBlocks = response.blocks
                .filter(b => b.height < oldestHeight)
                .sort((a, b) => b.height - a.height);

            if (olderBlocks.length > 0) {
                setBlocks(prev => [...prev, ...olderBlocks]);
                setHasMoreBlocks(olderBlocks[olderBlocks.length - 1].height > 1);
            } else {
                setHasMoreBlocks(false);
            }
        } catch (err) {
            console.error('Failed to fetch more blocks:', err);
        } finally {
            setIsLoadingMore(false);
        }
    }, [isLoadingMore, hasMoreBlocks, blocks]);

    // Initial fetch
    useEffect(() => {
        initialFetch();
    }, [initialFetch]);

    // Polling for new blocks (doesn't replace, only prepends)
    useEffect(() => {
        // Poll more frequently for responsive updates
        const interval = setInterval(pollForNewBlocks, 1500);

        // Listen for mining events to update instantly
        const handleMineEvent = () => {
            // 1. Immediately fetch from Bitcoin Core (optimistic)
            fetchLatestBlock();

            // 2. Poll Espo shortly after to get the trace data (eventual consistency)
            setTimeout(pollForNewBlocks, 500);
            setTimeout(pollForNewBlocks, 1500);
        };

        window.addEventListener('isomer:mine', handleMineEvent);

        return () => {
            clearInterval(interval);
            window.removeEventListener('isomer:mine', handleMineEvent);
        };
    }, [pollForNewBlocks, fetchLatestBlock]);

    // Fetch block details when selection changes
    useEffect(() => {
        if (!selectedBlock) {
            setSelectedBlockDetails(null);
            return;
        }

        const fetchDetails = async () => {
            try {
                // Keep show last details until new ones are loaded for better transition
                const details = await espo.getBlockDetails(selectedBlock.height);
                setSelectedBlockDetails(details);
            } catch (err) {
                console.error('Failed to fetch block details:', err);
            }
        };

        fetchDetails();
    }, [selectedBlock]);

    // Intersection Observer for infinite scroll
    useEffect(() => {
        const sentinel = sentinelRef.current;
        const container = scrollContainerRef.current;
        if (!sentinel || !container) return;

        const observer = new IntersectionObserver(
            (entries) => {
                if (entries[0].isIntersecting && !isLoadingMore && hasMoreBlocks) {
                    fetchMoreBlocks();
                }
            },
            {
                root: container,
                rootMargin: '100px',
                threshold: 0
            }
        );

        observer.observe(sentinel);
        return () => observer.disconnect();
    }, [fetchMoreBlocks, isLoadingMore, hasMoreBlocks]);

    // Keyboard navigation for block carousel
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Don't capture if user is typing in an input
            if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

            // Only handle if blocks exist
            if (blocks.length === 0) return;

            const currentIndex = focusedIndex >= 0 ? focusedIndex :
                (selectedBlock ? blocks.findIndex(b => b.height === selectedBlock.height) : -1);

            switch (e.key) {
                case 'ArrowRight':
                    e.preventDefault();
                    // Navigate to older block (higher index) - stop at end
                    if (currentIndex < blocks.length - 1) {
                        const nextIndex = currentIndex + 1;
                        setFocusedIndex(nextIndex);
                        setSelectedBlock(blocks[nextIndex]);
                        onBlockSelect?.();
                    }
                    break;
                case 'ArrowLeft':
                    e.preventDefault();
                    // Navigate to newer block (lower index) - stop at tip
                    if (currentIndex > 0) {
                        const prevIndex = currentIndex - 1;
                        setFocusedIndex(prevIndex);
                        setSelectedBlock(blocks[prevIndex]);
                        onBlockSelect?.();
                    }
                    break;
                case 'Home':
                    e.preventDefault();
                    // Jump to tip (newest)
                    setFocusedIndex(0);
                    setSelectedBlock(blocks[0]);
                    onBlockSelect?.();
                    scrollToLatest();
                    break;
                case 'End':
                    e.preventDefault();
                    // Jump to oldest visible
                    const lastIndex = blocks.length - 1;
                    setFocusedIndex(lastIndex);
                    setSelectedBlock(blocks[lastIndex]);
                    onBlockSelect?.();
                    break;
                case 'Enter':
                case ' ':
                    if (focusedIndex >= 0 && focusedIndex < blocks.length) {
                        e.preventDefault();
                        const block = blocks[focusedIndex];
                        // Toggle selection
                        if (selectedBlock?.height === block.height) {
                            setSelectedBlock(null);
                        } else {
                            setSelectedBlock(block);
                            onBlockSelect?.();
                        }
                    }
                    break;
                case 'Escape':
                    // Deselect
                    setSelectedBlock(null);
                    setFocusedIndex(-1);
                    break;
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [blocks, focusedIndex, selectedBlock, onBlockSelect, scrollToLatest]);

    // Use live block data for the detail view to ensure hydrated traces are shown
    const liveSelectedBlock = selectedBlock
        ? blocks.find(b => b.height === selectedBlock.height) || selectedBlock
        : null;

    const openInEspo = (height?: number) => {
        const url = height ? espo.getBlockUrl(height) : espo.getExplorerUrl();
        openUrl(url);
    };

    if (error) {
        return (
            <div className="h-full flex flex-col items-center justify-center text-zinc-500 gap-3 p-4">
                <div className="text-sm">{error}</div>
                <button
                    onClick={initialFetch}
                    className="flex items-center gap-2 px-3 py-1.5 text-xs bg-zinc-800 hover:bg-zinc-700 rounded-md transition-colors"
                >
                    <RefreshCw size={12} />
                    Retry
                </button>
            </div>
        );
    }

    return (
        <div className="h-full flex flex-col bg-zinc-950">
            {/* Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-zinc-800/50 flex-shrink-0">
                <div className="flex items-center gap-3">
                    <span className="text-[11px] font-medium text-zinc-500 uppercase tracking-wider">Chain Tip</span>
                    <div className="flex items-center gap-1.5">
                        <div className={`w-1.5 h-1.5 rounded-full transition-all ${isNewBlock ? 'bg-emerald-400 scale-150' : 'bg-emerald-500/60'}`} />
                        <span className="text-xs font-mono text-zinc-300">
                            #{espoTip.toLocaleString()}
                        </span>
                    </div>
                </div>

                <div className="flex items-center gap-3">
                    <span className="text-[9px] text-zinc-700 font-mono hidden sm:block">
                        E close · M {isMaximized ? 'minimize' : 'maximize'}
                    </span>
                    <button
                        onClick={() => openInEspo()}
                        className="flex items-center gap-1.5 px-2 py-1 text-[10px] font-medium text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 rounded transition-colors"
                    >
                        View in Espo
                        <ExternalLink size={10} />
                    </button>
                </div>
            </div>

            {/* Blocks Carousel Container with Scroll Button Overlay */}
            <div className="relative flex-1 min-h-0 bg-stone-900/10 flex flex-col">
                {/* Jump to Latest Button */}
                {showScrollLatest && (
                    <button
                        onClick={scrollToLatest}
                        className="absolute left-6 top-24 -translate-y-1/2 z-20 p-2.5 rounded-full bg-zinc-900/95 border border-amber-500/20 text-zinc-300 hover:text-amber-400 hover:bg-zinc-800 hover:scale-110 active:scale-95 shadow-[0_0_20px_rgba(0,0,0,0.5)] backdrop-blur-md transition-all duration-200 group"
                        title="Jump to Latest"
                    >
                        <ArrowLeftToLine size={16} />
                        <div className="absolute left-full ml-3 px-2 py-1 bg-zinc-800 text-[10px] text-zinc-300 rounded opacity-0 group-hover:opacity-100 transition-opacity whitespace-nowrap pointer-events-none border border-zinc-700 shadow-xl">
                            Jump to Latest
                        </div>
                    </button>
                )}

                <div
                    ref={scrollContainerRef}
                    role="listbox"
                    aria-label="Block carousel"
                    aria-orientation="horizontal"
                    tabIndex={-1}
                    className="flex-shrink-0 h-44 flex items-center px-4 gap-3 overflow-x-auto overflow-y-hidden scrollbar-hide py-4 border-b border-zinc-800/30 focus:outline-none"
                >
                    {/* Reverse Sentinel for infinite scroll - loading indicator */}
                    <div ref={sentinelRef} className="flex-shrink-0 w-8 flex items-center justify-center">
                        {isLoadingMore && <Loader size={14} className="animate-spin text-zinc-700" />}
                    </div>

                    {blocks.map((block) => (
                        <BlockCard
                            key={block.height}
                            block={block}
                            isTip={block.height === espoTip}
                            isSelected={selectedBlock?.height === block.height}
                            onClick={() => {
                                setSelectedBlock(selectedBlock?.height === block.height ? null : block);
                                if (selectedBlock?.height !== block.height) {
                                    onBlockSelect?.();
                                }
                            }}
                        />
                    ))}

                    {!hasMoreBlocks && blocks.length > 0 && (
                        <div className="flex-shrink-0 w-24 flex flex-col items-center justify-center text-[10px] text-zinc-700 bg-zinc-900/20 h-32 rounded-lg border border-dashed border-zinc-800/50">
                            <span>Genesis</span>
                        </div>
                    )}
                </div>

                {/* Inline Block Detail - only show when block is selected */}
                {liveSelectedBlock && (
                    <div className="flex-1 min-h-0 overflow-y-auto custom-scrollbar pt-4">
                        <BlockDetail
                            block={liveSelectedBlock}
                            details={liveSelectedBlock?.height === selectedBlockDetails?.height ? selectedBlockDetails : null}
                            isTip={liveSelectedBlock.height === espoTip}
                            onViewInEspo={() => openInEspo(liveSelectedBlock.height)}
                        />
                    </div>
                )}
            </div>
        </div>
    );
}

export default ExplorerPanel;
