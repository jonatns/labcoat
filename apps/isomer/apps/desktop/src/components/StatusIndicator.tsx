import type { ServiceStatus } from '../lib/types';

interface StatusIndicatorProps {
    status: ServiceStatus;
    size?: 'sm' | 'md' | 'lg';
}

export function StatusIndicator({ status, size = 'md' }: StatusIndicatorProps) {
    const sizeClasses = {
        sm: 'w-2 h-2',
        md: 'w-3 h-3',
        lg: 'w-4 h-4',
    };

    const getStatusColor = () => {
        if (status === 'running') return 'bg-emerald-500 status-running';
        if (status === 'starting') return 'bg-amber-500 status-starting';
        if (status === 'stopped') return 'bg-zinc-500';
        if (typeof status === 'object' && 'error' in status) return 'bg-rose-500';
        return 'bg-zinc-500';
    };

    const getStatusText = () => {
        if (status === 'running') return 'Running';
        if (status === 'starting') return 'Starting...';
        if (status === 'stopped') return 'Stopped';
        if (typeof status === 'object' && 'error' in status) return `Error: ${status.error}`;
        return 'Unknown';
    };

    return (
        <div className="flex items-center gap-2">
            <div className={`${sizeClasses[size]} ${getStatusColor()} rounded-full`} />
            <span className="text-sm text-zinc-400">{getStatusText()}</span>
        </div>
    );
}

export default StatusIndicator;
