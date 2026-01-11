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
        <div className="p-8 h-full flex flex-col">
            <div className="flex items-center justify-between mb-4">
                <div>
                    <h1 className="text-2xl font-bold text-white">Logs</h1>
                    <p className="text-zinc-400 mt-1">
                        {filteredLogs.length !== logs.length
                            ? `${filteredLogs.length} / ${logs.length} entries`
                            : `${logs.length} entries`
                        }
                    </p>
                </div>

                <div className="flex items-center gap-3">
                    <button
                        onClick={() => setIsAutoScroll(!isAutoScroll)}
                        className={`px-3 py-2 border rounded-lg text-sm transition-colors ${isAutoScroll
                            ? 'bg-indigo-600/20 border-indigo-600/50 text-indigo-400'
                            : 'bg-zinc-800 border-zinc-700 text-zinc-400 hover:bg-zinc-700'
                            }`}
                    >
                        {isAutoScroll ? '⏬ Auto' : '⏸️ Paused'}
                    </button>

                    <button
                        onClick={handleClear}
                        className="px-3 py-2 bg-red-600/20 hover:bg-red-600/30 border border-red-600/50 
                                 rounded-lg text-red-400 text-sm transition-colors"
                    >
                        Clear
                    </button>
                </div>
            </div>

            {/* Service filter toggles */}
            <div className="flex flex-wrap items-center gap-2 mb-4">
                <button
                    onClick={toggleAll}
                    className={`px-2 py-1 text-xs font-medium rounded border transition-colors ${enabledServices.size === ALL_SERVICES.length
                        ? 'bg-zinc-700 border-zinc-600 text-white'
                        : 'bg-zinc-800/50 border-zinc-700 text-zinc-500 hover:border-zinc-600'
                        }`}
                >
                    All
                </button>
                {ALL_SERVICES.map(service => (
                    <button
                        key={service}
                        onClick={() => toggleService(service)}
                        className={`px-2 py-1 text-xs font-medium rounded border transition-colors ${enabledServices.has(service)
                            ? SERVICE_COLORS[service] || 'bg-zinc-700 text-white border-zinc-600'
                            : 'bg-zinc-800/50 border-zinc-700 text-zinc-600 hover:border-zinc-600'
                            }`}
                    >
                        {service}
                    </button>
                ))}
            </div>

            {/* Log output */}
            <div
                ref={containerRef}
                onScroll={handleScroll}
                className="flex-1 glass rounded-xl p-4 overflow-auto font-mono text-xs"
            >
                {filteredLogs.length === 0 ? (
                    <div className="text-zinc-500 text-center py-8">
                        <p>{logs.length === 0 ? 'No logs yet. Start services to see output.' : 'No logs match the selected filters.'}</p>
                    </div>
                ) : (
                    <div className="space-y-0.5">
                        {filteredLogs.map((log, index) => (
                            <div
                                key={`${log.timestamp}-${index}`}
                                className={`flex gap-2 py-0.5 ${log.is_stderr ? 'text-amber-400' : 'text-zinc-300'}`}
                            >
                                <span className="text-zinc-600 shrink-0">{formatTime(log.timestamp)}</span>
                                <span className={`shrink-0 px-1.5 rounded text-[10px] font-medium ${SERVICE_COLORS[log.service]?.replace('border-', 'border border-') || 'bg-zinc-700 text-zinc-400'
                                    }`}>
                                    {log.service}
                                </span>
                                <span className="break-all">{log.message}</span>
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
