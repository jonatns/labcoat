import { useState, useEffect, useRef } from 'react';
import { api } from '../lib/api';
import type { LogEntry } from '../lib/types';

const SERVICE_COLORS: Record<string, string> = {
    'bitcoind': 'bg-orange-600/20 text-orange-400 border-orange-600/50',
    'metashrew': 'bg-purple-600/20 text-purple-400 border-purple-600/50',
    'ord': 'bg-cyan-600/20 text-cyan-400 border-cyan-600/50',
    'esplora': 'bg-blue-600/20 text-blue-400 border-blue-600/50',
    'espo': 'bg-teal-600/20 text-teal-400 border-teal-600/50',
    'jsonrpc': 'bg-green-600/20 text-green-400 border-green-600/50',
};

const ALL_SERVICES = ['bitcoind', 'metashrew', 'ord', 'esplora', 'espo', 'jsonrpc'];

export function LogsPanel() {
    const [enabledServices, setEnabledServices] = useState<Set<string>>(new Set(ALL_SERVICES));
    const [logs, setLogs] = useState<LogEntry[]>([]);
    const [isAutoScroll, setIsAutoScroll] = useState(true);
    const logsEndRef = useRef<HTMLDivElement>(null);
    const containerRef = useRef<HTMLDivElement>(null);

    // Toggle a service filter
    const toggleService = (service: string) => {
        setEnabledServices(prev => {
            // If "All" are currently enabled, clicking one "solos" it
            if (prev.size === ALL_SERVICES.length) {
                return new Set([service]);
            }

            // Otherwise toggle normally
            const next = new Set(prev);
            if (next.has(service)) {
                // Prevent deselecting the last one
                if (next.size > 1) {
                    next.delete(service);
                }
            } else {
                next.add(service);
            }

            // If all became selected manually, it's effectively "All"
            return next;
        });
    };

    // Toggle all services
    const toggleAll = () => {
        if (enabledServices.size === ALL_SERVICES.length) {
            setEnabledServices(new Set());
        } else {
            setEnabledServices(new Set(ALL_SERVICES));
        }
    };

    // Poll for logs
    useEffect(() => {
        const fetchLogs = async () => {
            try {
                const newLogs = await api.getLogs(undefined, 500);
                setLogs(newLogs);
            } catch (err) {
                console.error('Failed to fetch logs:', err);
            }
        };

        fetchLogs();
        const interval = setInterval(fetchLogs, 1000);
        return () => clearInterval(interval);
    }, []);

    // Filter logs by enabled services
    const filteredLogs = logs.filter(log => enabledServices.has(log.service));
    const isScrollingRef = useRef(false);

    // Auto-scroll to bottom when new logs arrive
    useEffect(() => {
        if (isAutoScroll && containerRef.current) {
            isScrollingRef.current = true;
            containerRef.current.scrollTop = containerRef.current.scrollHeight;
            // Reset after a short delay
            setTimeout(() => {
                isScrollingRef.current = false;
            }, 100);
        }
    }, [filteredLogs.length, isAutoScroll]);

    // Detect manual scroll to disable auto-scroll
    const handleScroll = () => {
        // Ignore scroll events during programmatic scrolling
        if (isScrollingRef.current) return;

        if (containerRef.current) {
            const { scrollTop, scrollHeight, clientHeight } = containerRef.current;
            const isAtBottom = scrollHeight - scrollTop - clientHeight < 50;
            setIsAutoScroll(isAtBottom);
        }
    };

    const handleClear = async () => {
        try {
            await api.clearLogs();
            setLogs([]);
        } catch (err) {
            console.error('Failed to clear logs:', err);
        }
    };

    const formatTime = (timestamp: number) => {
        const date = new Date(timestamp * 1000);
        return date.toLocaleTimeString();
    };

    return (
        <div className="flex flex-col h-full bg-zinc-950/30">
            {/* Compact Toolbar */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-zinc-800/50 bg-zinc-900/20 shrink-0">
                {/* Left: Service Filters */}
                <div className="flex flex-wrap items-center gap-1.5">
                    <button
                        onClick={toggleAll}
                        className={`px-2 py-0.5 text-[10px] font-medium rounded border transition-colors ${enabledServices.size === ALL_SERVICES.length
                            ? 'bg-zinc-700 border-zinc-600 text-white'
                            : 'bg-zinc-800/50 border-zinc-700 text-zinc-500 hover:border-zinc-600'
                            }`}
                    >
                        ALL
                    </button>
                    <div className="w-px h-3 bg-zinc-800 mx-1" />
                    {ALL_SERVICES.map(service => (
                        <button
                            key={service}
                            onClick={() => toggleService(service)}
                            className={`px-2 py-0.5 text-[10px] uppercase font-bold rounded border transition-colors ${enabledServices.has(service)
                                ? SERVICE_COLORS[service] || 'bg-zinc-700 text-white border-zinc-600'
                                : 'bg-zinc-800/50 border-zinc-700 text-zinc-600 hover:border-zinc-600'
                                }`}
                        >
                            {service}
                        </button>
                    ))}
                </div>

                {/* Right: Actions & Stats */}
                <div className="flex items-center gap-3">
                    <span className="text-[10px] text-zinc-500 font-mono">
                        {filteredLogs.length !== logs.length
                            ? `${filteredLogs.length}/${logs.length}`
                            : `${logs.length}`
                        } lines
                    </span>

                    <div className="w-px h-3 bg-zinc-800" />

                    <button
                        onClick={() => setIsAutoScroll(!isAutoScroll)}
                        className={`px-2 py-0.5 border rounded text-[10px] font-medium transition-colors ${isAutoScroll
                            ? 'bg-indigo-500/10 border-indigo-500/30 text-indigo-400'
                            : 'bg-zinc-800 border-zinc-700 text-zinc-500 hover:bg-zinc-700'
                            }`}
                    >
                        {isAutoScroll ? 'AUTO-SCROLL' : 'PAUSED'}
                    </button>

                    <button
                        onClick={handleClear}
                        className="px-2 py-0.5 bg-red-500/10 hover:bg-red-500/20 border border-red-500/20 
                                 rounded text-red-400 text-[10px] font-medium transition-colors"
                    >
                        CLEAR
                    </button>
                </div>
            </div>

            {/* Log output */}
            <div
                ref={containerRef}
                onScroll={handleScroll}
                className="flex-1 overflow-auto font-mono text-xs p-4 scrollbar-thin scrollbar-thumb-zinc-700 scrollbar-track-transparent"
            >
                {filteredLogs.length === 0 ? (
                    <div className="h-full flex flex-col items-center justify-center text-zinc-600 space-y-2 opacity-50">
                        <div className="w-12 h-1 bg-zinc-800/50 rounded-full" />
                        <p className="text-sm">No active logs</p>
                    </div>
                ) : (
                    <div className="space-y-0.5">
                        {filteredLogs.map((log, index) => (
                            <div
                                key={`${log.timestamp}-${index}`}
                                className={`flex gap-3 py-0.5 hover:bg-white/5 transition-colors ${log.is_stderr ? 'text-amber-400/90' : 'text-zinc-300'}`}
                            >
                                <span className="text-zinc-600 shrink-0 font-mono text-[10px] pt-0.5 opacity-50 select-none block w-14">{formatTime(log.timestamp)}</span>
                                <span className={`shrink-0 px-1 py-px rounded-[2px] text-[9px] font-bold uppercase tracking-wider h-fit ${SERVICE_COLORS[log.service]?.replace('border-', 'border border-') || 'bg-zinc-700 text-zinc-400'
                                    }`}>
                                    {log.service}
                                </span>
                                <span className="break-all whitespace-pre-wrap leading-tight font-mono text-[11px] opacity-90">{log.message}</span>
                            </div>
                        ))}
                    </div>
                )}
                <div ref={logsEndRef} />
            </div>
        </div>
    );
}

export default LogsPanel;
