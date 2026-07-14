import { Cpu, Pickaxe, Play, Square, Loader } from 'lucide-react';
import { useState } from 'react';
import { DevFaucet } from './DevFaucet';

interface ControlPlaneProps {
    serviceStatus: 'stopped' | 'starting' | 'running' | 'error' | 'stopping';
    isMining: boolean;
    onMine: () => void;
    onStartStop: () => void;
}

export function ControlPlane({ serviceStatus, isMining, onMine, onStartStop }: ControlPlaneProps) {
    const isRunning = serviceStatus === 'running';
    const isStarting = serviceStatus === 'starting';
    const isStopping = serviceStatus === 'stopping';

    // Animation States
    const [sparkleActive, setSparkleActive] = useState(false);
    const [shockwaveActive, setShockwaveActive] = useState(false);

    const handleMine = () => {
        if (!isRunning || isMining) return;

        // Trigger animations
        setSparkleActive(true);
        setShockwaveActive(true);
        setTimeout(() => setSparkleActive(false), 800);
        setTimeout(() => setShockwaveActive(false), 600);

        onMine();
    };

    const handleStartClick = () => {
        if (!isStarting && !isStopping) {
            onStartStop();
        }
    };

    return (
        <div className="pt-24 pb-12 px-6 border-b border-zinc-800/50 bg-gradient-to-b from-zinc-900/50 to-transparent relative overflow-visible">

            <div className="flex items-center justify-center gap-8 relative z-10 w-full max-w-2xl mx-auto">

                {/* Quick Action: Start/Stop Override */}
                <div className="flex flex-col items-center gap-3 w-32">
                    <button
                        onClick={handleStartClick}
                        className={`
                            relative w-14 h-14 rounded-2xl flex items-center justify-center border transition-all duration-300
                            ${isStarting || isStopping
                                ? 'bg-amber-500/20 border-amber-500/50 text-amber-500 cursor-wait'
                                : isRunning
                                    ? 'bg-rose-500/10 border-rose-500/30 text-rose-500 hover:bg-rose-500/20'
                                    : 'bg-zinc-800/50 border-zinc-700 text-zinc-400 hover:border-zinc-500 hover:bg-zinc-800 hover:text-emerald-400 active:scale-95'}
                        `}
                    >
                        {isStarting || isStopping ? (
                            <Loader className="w-6 h-6 animate-spin" />
                        ) : isRunning ? (
                            <Square className="w-6 h-6 fill-rose-500" />
                        ) : (
                            <Play className="w-6 h-6 fill-current ml-1" />
                        )}
                    </button>
                    <span className={`text-[10px] text-center font-bold tracking-widest uppercase transition-colors whitespace-nowrap ${isStarting || isStopping ? 'text-amber-500' : 'text-zinc-500'}`}>
                        {isStarting ? 'Initializing...' : isStopping ? 'Stopping...' : isRunning ? 'Stop System' : 'Start System'}
                    </span>
                </div>


                {/* THE REACTOR (Central Status) */}
                <div className="relative group overflow-visible flex-shrink-0 pb-6">
                    {/* Shockwave Effect (Mining) */}
                    {shockwaveActive && (
                        <div className="absolute inset-0 rounded-full border-4 border-amber-500/50 animate-shockwave z-0 pointer-events-none will-change-transform" />
                    )}

                    {/* Ambient Glow - Using Radial Gradient to fix 'Square' artifact */}
                    <div
                        className={`
                            absolute inset-[-100px] pointer-events-none transition-opacity duration-1000
                            ${isRunning ? 'opacity-100' : 'opacity-0'}
                        `}
                        style={{
                            background: 'radial-gradient(circle, rgba(16, 185, 129, 0.15) 0%, rgba(16, 185, 129, 0) 60%)'
                        }}
                    />

                    <div className={`
                        relative w-36 h-36 rounded-full border-4 flex items-center justify-center transition-all duration-700 z-10 will-change-transform
                        ${isRunning
                            ? 'border-emerald-500/20 bg-zinc-950 animate-reactor-breathe'
                            : 'border-zinc-800 bg-zinc-900/50'}
                    `}>
                        {/* Spinning Rings (Complex) */}
                        {isRunning && (
                            <>
                                {/* Outer Ring - Slow */}
                                <div className="absolute inset-[-6px] rounded-full border border-emerald-500/30 border-t-transparent border-l-transparent animate-spin duration-[8000ms] will-change-transform" />

                                {/* Inner Ring - Medium Reverse */}
                                <div className="absolute inset-2 rounded-full border-2 border-dashed border-emerald-500/20 animate-spin duration-[12000ms] direction-reverse will-change-transform" />

                                {/* Core Ring - Fast */}
                                <div className="absolute inset-8 rounded-full border border-emerald-500/40 border-b-transparent animate-spin duration-[3000ms] will-change-transform" />
                            </>
                        )}

                        <Cpu className={`
                            w-12 h-12 transition-all duration-500
                            ${isRunning ? 'text-emerald-400 drop-shadow-[0_0_8px_rgba(52,211,153,0.5)]' : 'text-zinc-700'}
                        `} strokeWidth={1.5} />
                    </div>

                    {/* Status Label */}
                    <div className="absolute -bottom-10 left-1/2 -translate-x-1/2 text-center w-full">
                        <span className={`
                            text-xs font-mono font-bold tracking-widest uppercase transition-colors duration-500
                            ${isRunning ? 'text-emerald-500' : 'text-zinc-600'}
                        `}>
                            {serviceStatus}
                        </span>
                    </div>
                </div>

                {/* Primary Action: Mine */}
                <div className="relative w-32 flex flex-col items-center">
                    {/* Sparkle Particles (simple implementation) */}
                    {sparkleActive && (
                        <div className="absolute top-0 left-1/2 -translate-x-1/2 pointer-events-none z-50">
                            <div className="w-1.5 h-1.5 bg-amber-400 rounded-full animate-fly-particle box-shadow-[0_0_10px_rgba(251,191,36,0.8)]" />
                        </div>
                    )}

                    <button
                        disabled={!isRunning || isMining}
                        onClick={handleMine}
                        className={`
                            group flex flex-col items-center gap-3 transition-all duration-100 active:scale-[0.98]
                            ${!isRunning ? 'opacity-30 grayscale cursor-not-allowed' : 'opacity-100 cursor-pointer'}
                        `}
                    >
                        <div className={`
                            w-14 h-14 rounded-2xl flex items-center justify-center border transition-all duration-200 relative
                            ${isMining
                                ? 'bg-amber-500/20 border-amber-500 text-amber-500'
                                : 'bg-gradient-to-br from-amber-500/10 to-transparent border-amber-500/30 text-amber-500 hover:border-amber-400 hover:bg-amber-500/10 hover:shadow-[0_0_20px_-5px_rgba(245,158,11,0.3)]'}
                        `}>
                            <Pickaxe className={`w-6 h-6 ${isMining ? 'animate-pulse' : 'transition-transform'}`} />
                        </div>
                        <span className="text-xs font-bold tracking-wide text-amber-500 uppercase group-hover:text-amber-400 whitespace-nowrap">
                            {isMining ? 'Mining...' : 'Mine Block'}
                        </span>
                    </button>

                    {/* Shortcut Hint */}
                    {isRunning && (
                        <span className="absolute -top-3 right-8 text-[9px] text-zinc-600 font-mono opacity-0 group-hover:opacity-100 transition-opacity">⌘⏎</span>
                    )}
                </div>

            </div>

            {/* Dev Faucet */}
            <DevFaucet isRunning={isRunning} />

        </div>
    );
}
