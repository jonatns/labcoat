import type { ServiceInfo } from '../lib/types';
import { useStore } from '../lib/store';
import { StatusIndicator } from './StatusIndicator';

interface ServiceCardProps {
    service: ServiceInfo;
}

export function ServiceCard({ service }: ServiceCardProps) {
    const { serviceHealth } = useStore();
    const isHealthy = serviceHealth[service.id];

    const formatUptime = (secs: number | null) => {
        if (secs === null) return '--';
        const hours = Math.floor(secs / 3600);
        const mins = Math.floor((secs % 3600) / 60);
        const s = secs % 60;
        if (hours > 0) return `${hours}h ${mins}m`;
        if (mins > 0) return `${mins}m ${s}s`;
        return `${s}s`;
    };

    const copyToClipboard = async (text: string) => {
        try {
            await navigator.clipboard.writeText(text);
            // Could add toast here, but for now simple feedback
        } catch (err) {
            console.error('Failed to copy code:', err);
        }
    };

    return (
        <div className="glass rounded-xl p-5 hover:border-zinc-500 transition-colors">
            <div className="flex items-center justify-between mb-4">
                <h3 className="font-semibold text-white text-lg">{service.name}</h3>
                <StatusIndicator status={service.status} />
            </div>

            <div className="space-y-3">
                <div className="flex items-center justify-between p-2 rounded bg-zinc-900/50 border border-zinc-800">
                    <span className="text-zinc-500 text-sm">Port</span>
                    <button
                        onClick={() => copyToClipboard(service.port.toString())}
                        className="text-zinc-300 font-mono text-sm hover:text-white transition-colors flex items-center gap-2 group"
                        title="Click to copy port"
                    >
                        {service.port}
                        <svg className="w-3 h-3 opacity-0 group-hover:opacity-100 transition-opacity" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                        </svg>
                    </button>
                </div>

                <div className="grid grid-cols-2 gap-3 text-sm">
                    <div>
                        <span className="text-zinc-500">PID</span>
                        <p className="text-zinc-300 font-mono">{service.pid ?? '--'}</p>
                    </div>
                    <div>
                        <span className="text-zinc-500">Version</span>
                        <p className="text-zinc-300 font-mono truncate" title={service.version || ''}>{service.version || 'unknown'}</p>
                    </div>
                    <div className="col-span-2">
                        <span className="text-zinc-500">Uptime</span>
                        <p className="text-zinc-300">{formatUptime(service.uptime_secs)}</p>
                    </div>
                </div>

                {service.status === 'running' && (
                    <div className="flex items-center gap-2 mt-2 pt-2 border-t border-zinc-800">
                        <div className={`w-2 h-2 rounded-full ${isHealthy ? 'bg-green-500' : 'bg-red-500'}`} />
                        <span className="text-zinc-400 text-sm">
                            {isHealthy ? 'Responding' : 'Not Responding'}
                        </span>
                    </div>
                )}
            </div>
        </div>
    );
}


export default ServiceCard;
