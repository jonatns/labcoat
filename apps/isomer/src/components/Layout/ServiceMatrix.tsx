import { CheckCircle2, Circle, Clock, Server, Layers, Terminal, AlertCircle, Loader2, Hexagon } from 'lucide-react';
import type { ServiceInfo } from '../../lib/types';

interface ServiceMatrixProps {
    status: 'stopped' | 'starting' | 'running' | 'error';
    services: ServiceInfo[];
    extensionStatus?: {
        installed: boolean;
        onOpen: () => void;
    };
}

const getServiceIcon = (id: string) => {
    switch (id) {
        case 'bitcoind': return <Layers className="w-4 h-4" />;
        case 'metashrew': return <Server className="w-4 h-4" />;
        case 'alkanes': return <Terminal className="w-4 h-4" />;
        default: return <Server className="w-4 h-4" />;
    }
};

const getServiceLabel = (id: string) => {
    switch (id) {
        case 'bitcoind': return 'Bitcoin Core';
        case 'metashrew': return 'Metashrew Indexer';
        case 'alkanes': return 'Alkanes RPC';
        default: return id;
    }
};

export function ServiceMatrix({ services, extensionStatus }: ServiceMatrixProps) {
    return (
        <div className="flex-1 overflow-y-auto px-6 py-4">
            <div className="grid gap-3 max-w-4xl mx-auto">
                {services.map((service) => {
                    const isRunning = service.status === 'running';
                    const isError = typeof service.status === 'object' && 'error' in service.status;
                    const isStarting = service.status === 'starting';

                    return (
                        <div
                            key={service.id}
                            className={`
                group flex items-center justify-between p-3 rounded-lg border transition-all duration-200
                ${isRunning
                                    ? 'bg-zinc-900/40 border-zinc-800 hover:border-zinc-700 hover:bg-zinc-900/60'
                                    : 'bg-zinc-900/20 border-zinc-800/50 opacity-60'}
              `}
                        >
                            {/* Left: Identity */}
                            <div className="flex items-center gap-4">
                                <div className={`
                   p-2 rounded-md transition-colors
                   ${isRunning ? 'bg-zinc-800 text-zinc-300' : 'bg-zinc-900 text-zinc-600'}
                 `}>
                                    {getServiceIcon(service.id)}
                                </div>
                                <div>
                                    <h3 className="text-sm font-medium text-zinc-200">{getServiceLabel(service.id)}</h3>
                                    <div className="flex items-center gap-2 mt-0.5">
                                        <span className="text-[10px] bg-zinc-800 px-1.5 py-0.5 rounded text-zinc-500 font-mono">
                                            {service.version || 'latest'}
                                        </span>
                                        <span className="text-[10px] text-zinc-600 font-mono">:{service.port}</span>
                                    </div>
                                </div>
                            </div>

                            {/* Right: Status & Actions */}
                            <div className="flex items-center gap-6">
                                {/* Latency Metric (only if running) */}
                                {isRunning && (
                                    <div className="flex items-center gap-1.5 text-xs text-zinc-500 font-mono">
                                        <Clock className="w-3 h-3" />
                                        <span>{service.uptime_secs ? `${(service.uptime_secs / 60).toFixed(0)}m` : '-'}</span>
                                    </div>
                                )}

                                {/* Quick Actions (Hover Reveal) */}
                                <div className="flex items-center gap-2 opacity-0 group-hover:opacity-100 transition-opacity duration-200">
                                    <button className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300 transition-colors" title="View Logs">
                                        <Layers className="w-3 h-3" />
                                    </button>
                                    <button className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300 transition-colors" title="Restart Service">
                                        <Terminal className="w-3 h-3" />
                                    </button>
                                </div>

                                {/* Status Pill */}
                                <div className={`
                    flex items-center gap-1.5 px-2 py-1 rounded-full text-[10px] font-bold tracking-wider uppercase border
                    ${isRunning
                                        ? 'bg-emerald-500/10 border-emerald-900/30 text-emerald-500'
                                        : isStarting
                                            ? 'bg-amber-500/10 border-amber-900/30 text-amber-500'
                                            : isError
                                                ? 'bg-rose-500/10 border-rose-900/30 text-rose-500'
                                                : 'bg-zinc-800 border-zinc-700 text-zinc-500'}
                 `}>
                                    {isRunning && <CheckCircle2 className="w-3 h-3" />}
                                    {isStarting && <Loader2 className="w-3 h-3 animate-spin" />}
                                    {isError && <AlertCircle className="w-3 h-3" />}
                                    {!isRunning && !isStarting && !isError && <Circle className="w-3 h-3" />}

                                    {/* Hide 'Online' text, show others */}
                                    {!isRunning && (isStarting ? 'Starting' : isError ? 'Error' : 'Offline')}
                                </div>
                            </div>
                        </div>
                    );
                })}

                {/* Companion Service Row */}
                {extensionStatus && (
                    <div
                        className={`
                            group flex items-center justify-between p-3 rounded-lg border transition-all duration-200
                            bg-zinc-900/10 border-zinc-800/30 opacity-50 cursor-not-allowed
                        `}
                    >
                        {/* Left: Identity */}
                        <div className="flex items-center gap-4">
                            <div className="p-2 rounded-md bg-zinc-900/50 text-zinc-600">
                                <Hexagon className="w-4 h-4" />
                            </div>
                            <div>
                                <h3 className="text-sm font-medium text-zinc-400">Isomer Companion</h3>
                                <div className="flex items-center gap-2 mt-0.5">
                                    <span className="text-[10px] text-zinc-600 font-mono">Browser Bridge</span>
                                </div>
                            </div>
                        </div>

                        {/* Right: Status & Actions */}
                        <div className="flex items-center gap-6">
                            {/* Status Pill */}
                            <div className="flex items-center gap-1.5 px-2 py-1 rounded-full text-[10px] font-bold tracking-wider uppercase border bg-blue-500/5 border-blue-500/10 text-blue-500/50">
                                <Clock className="w-3 h-3" />
                                Coming Soon
                            </div>

                            {/* Action Affordance - Hidden/Disabled */}
                            <div className="w-4 h-4" />
                        </div>
                    </div>
                )}


                {services.length === 0 && !extensionStatus && (
                    <div className="text-center py-10 text-zinc-500 text-sm">
                        No services detected. System may be initializing.
                    </div>
                )}
            </div>
        </div>
    );
}
