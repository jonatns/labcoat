import type { ServiceInfo } from '../lib/types';
import { StatusIndicator } from './StatusIndicator';

interface ServiceCardProps {
    service: ServiceInfo;
}

export function ServiceCard({ service }: ServiceCardProps) {
    const formatUptime = (secs: number | null) => {
        if (secs === null) return '--';
        const hours = Math.floor(secs / 3600);
        const mins = Math.floor((secs % 3600) / 60);
        const s = secs % 60;
        if (hours > 0) return `${hours}h ${mins}m`;
        if (mins > 0) return `${mins}m ${s}s`;
        return `${s}s`;
    };

    return (
        <div className="glass rounded-xl p-5 hover:border-zinc-500 transition-colors">
            <div className="flex items-center justify-between mb-4">
                <h3 className="font-semibold text-white text-lg">{service.name}</h3>
                <StatusIndicator status={service.status} />
            </div>

            <div className="grid grid-cols-2 gap-3 text-sm">
                <div>
                    <span className="text-zinc-500">Port</span>
                    <p className="text-zinc-300 font-mono text-base">{service.port}</p>
                </div>
                <div>
                    <span className="text-zinc-500">PID</span>
                    <p className="text-zinc-300 font-mono text-base">{service.pid ?? '--'}</p>
                </div>
                <div className="col-span-2 mt-1">
                    <span className="text-zinc-500">Uptime</span>
                    <p className="text-zinc-300 text-base">{formatUptime(service.uptime_secs)}</p>
                </div>
            </div>
        </div>
    );
}

export default ServiceCard;
