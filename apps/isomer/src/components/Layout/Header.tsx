import { Activity, Box } from 'lucide-react';
import { useEffect, useState } from 'react';

interface HeaderProps {
    blockHeight: number;
    mempoolSize: number;
    isSystemHealth: boolean;
}

function TelemetryValue({ value, unit = '' }: { value: number | string, unit?: string }) {
    const [animating, setAnimating] = useState(false);

    useEffect(() => {
        setAnimating(true);
        const timer = setTimeout(() => setAnimating(false), 600);
        return () => clearTimeout(timer);
    }, [value]);

    return (
        <span className={`font-mono text-sm text-zinc-200 transition-colors ${animating ? 'animate-text-flash font-bold' : ''}`}>
            {typeof value === 'number' ? value.toLocaleString() : value} {unit}
        </span>
    );
}

export function Header({ blockHeight, mempoolSize, isSystemHealth }: HeaderProps) {
    return (
        <header
            data-tauri-drag-region
            className="h-10 bg-zinc-950/60 backdrop-blur-md border-b border-white/5 flex items-center justify-between px-4 select-none z-50 transition-all duration-300 cursor-default"
        >
            {/* Left: Window Controls & Brand */}
            {/* z-50 ensures content is clickable over the drag region */}
            <div className="flex items-center gap-4 relative z-50 pl-22">
                <div className="flex items-center gap-2 group">
                    <span className="font-mono font-bold text-sm tracking-widest text-zinc-100 flex items-center gap-2">
                        ISOMER <span className="text-zinc-600 font-normal">v0.1.1</span>
                    </span>
                </div>

                {/* System Heartbeat Pill */}
                <div className={`
          flex items-center gap-2 px-3 py-1 rounded-full text-xs font-mono border transition-all duration-500
          ${isSystemHealth
                        ? 'bg-emerald-500/10 border-emerald-500/20 text-emerald-400 animate-pulse-glow-success'
                        : 'bg-zinc-900 border-zinc-800 text-zinc-500'}
        `}>
                    <div className={`w-2 h-2 rounded-full ${isSystemHealth ? 'bg-emerald-500 animate-pulse' : 'bg-zinc-600'}`} />
                    {isSystemHealth ? 'REGTEST ACTIVE' : 'SYSTEM OFFLINE'}
                </div>
            </div>

            {/* Right: Telemetry */}
            <div className="flex items-center gap-6 relative z-50">
                <div className="flex items-center gap-2 text-zinc-400">
                    <Box className="w-4 h-4 text-zinc-600" />
                    <div className="flex flex-col items-end leading-none">
                        <span className="text-[10px] uppercase text-zinc-600 font-bold tracking-wider">Height</span>
                        <TelemetryValue value={blockHeight} />
                    </div>
                </div>

                <div className="w-px h-8 bg-zinc-800" />

                <div className="flex items-center gap-2 text-zinc-400">
                    <Activity className="w-4 h-4 text-zinc-600" />
                    <div className="flex flex-col items-end leading-none">
                        <span className="text-[10px] uppercase text-zinc-600 font-bold tracking-wider">Mempool</span>
                        <TelemetryValue value={mempoolSize} unit="tx" />
                    </div>
                </div>
            </div>
        </header>
    );
}
