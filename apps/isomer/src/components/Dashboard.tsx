import { useState, useEffect } from 'react';
import { ServiceCard } from './ServiceCard';
import { MiningPanel } from './MiningPanel';
import { FaucetPanel } from './FaucetPanel';
import { useStore } from '../lib/store';
import { api } from '../lib/api';
import { useBinaries } from '../hooks/useStatus';

export function Dashboard() {
    const { status, error, setError } = useStore();
    const [isStarting, setIsStarting] = useState(false);
    const [isStopping, setIsStopping] = useState(false);
    const [copied, setCopied] = useState(false);
    const { binaries, checkBinaries } = useBinaries();

    useEffect(() => {
        checkBinaries();
    }, []);

    const missingBinaries = binaries.some(b => b.status === 'notinstalled');

    const handleStart = async () => {
        setIsStarting(true);
        setError(null);
        try {
            await api.startServices();
        } catch (err) {
            console.error('Failed to start services:', err);
            setError(err instanceof Error ? err.message : String(err));
        } finally {
            setIsStarting(false);
        }
    };

    const handleStop = async () => {
        setIsStopping(true);
        try {
            await api.stopServices();
        } catch (err) {
            console.error('Failed to stop services:', err);
        } finally {
            setIsStopping(false);
        }
    };



    if (!status) {
        return (
            <div className="flex items-center justify-center h-full">
                <div className="text-center">
                    <div className="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto mb-4" />
                    <p className="text-zinc-400">Loading...</p>
                </div>
            </div>
        );
    }

    const allRunning = status.services.every((s) => s.status === 'running');
    const allStopped = status.services.every((s) => s.status === 'stopped');

    return (
        <div className="p-8 space-y-8 overflow-auto h-full">
            {/* Header */}
            <div className="flex items-center justify-between">
                <div>
                    <h1 className="text-2xl font-bold text-white">Dashboard</h1>
                    <p className="text-zinc-400 mt-1">
                        {allRunning ? 'All services running' : allStopped ? 'Services stopped' : 'Services partially running'}
                    </p>
                </div>

                <div className="flex items-center gap-3">

                    <button
                        onClick={handleStop}
                        disabled={allStopped || isStopping}
                        className="px-4 py-2 bg-red-600/20 hover:bg-red-600/30 disabled:opacity-50 
                       disabled:cursor-not-allowed border border-red-600/50 rounded-lg 
                       text-red-400 font-medium transition-colors"
                    >
                        {isStopping ? 'Stopping...' : 'Stop All'}
                    </button>

                    <button
                        onClick={handleStart}
                        disabled={allRunning || isStarting || missingBinaries}
                        className="px-4 py-2 bg-green-600 hover:bg-green-500 disabled:opacity-50 
                       disabled:cursor-not-allowed rounded-lg text-white font-medium 
                       transition-colors"
                    >
                        {isStarting ? 'Starting...' : 'Start All'}
                    </button>
                </div>
            </div>

            {/* Error banner */}
            {error && (
                <div className="bg-red-600/20 border border-red-600/50 rounded-lg px-4 py-3 text-red-400">
                    {error}
                </div>
            )}

            {/* RPC Endpoint - Prominent display like Ganache */}
            <div className="glass rounded-xl p-4 flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <div className="w-10 h-10 rounded-lg bg-indigo-500/20 flex items-center justify-center">
                        <svg className="w-5 h-5 text-indigo-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2" />
                        </svg>
                    </div>
                    <div>
                        <p className="text-zinc-400 text-xs uppercase tracking-wider">RPC Server</p>
                        <p className="text-xl font-mono text-white">http://127.0.0.1:18888</p>
                    </div>
                </div>
                <button
                    onClick={async () => {
                        await navigator.clipboard.writeText('http://127.0.0.1:18888');
                        setCopied(true);
                        setTimeout(() => setCopied(false), 2000);
                    }}
                    className={`px-4 py-2 border rounded-lg font-medium transition-all flex items-center gap-2 ${copied
                        ? 'bg-green-500/20 border-green-500/50 text-green-400'
                        : 'bg-indigo-500/20 hover:bg-indigo-500/30 border-indigo-500/50 text-indigo-400'
                        }`}
                    title="Copy to clipboard"
                >
                    {copied ? (
                        <>
                            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
                            </svg>
                            Copied!
                        </>
                    ) : (
                        <>
                            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                            </svg>
                            Copy
                        </>
                    )}
                </button>
            </div>

            {/* Quick Stats */}
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-6">
                <div className="glass rounded-xl p-5">
                    <p className="text-zinc-500 text-sm mb-1">Block Height</p>
                    <p className="text-3xl font-mono text-indigo-400">{status.block_height}</p>
                </div>
                <div className="glass rounded-xl p-5">
                    <p className="text-zinc-500 text-sm mb-1">Mempool</p>
                    <p className="text-3xl font-mono text-cyan-400">{status.mempool_size} txs</p>
                </div>
                <div className="glass rounded-xl p-5">
                    <p className="text-zinc-500 text-sm mb-1">Services</p>
                    <p className="text-3xl font-mono text-green-400">
                        {status.services.filter((s) => s.status === 'running').length}/{status.services.length}
                    </p>
                </div>
                <div className="glass rounded-xl p-5">
                    <p className="text-zinc-500 text-sm mb-1">Status</p>
                    <p className={`text-3xl font-medium ${status.is_ready
                            ? 'text-green-400'
                            : status.services.some((s) => s.status === 'running')
                                ? 'text-amber-400'
                                : 'text-zinc-400'
                        }`}>
                        {status.is_ready
                            ? 'Ready'
                            : status.services.some((s) => s.status === 'running')
                                ? 'Starting'
                                : 'Stopped'}
                    </p>
                </div>
            </div>

            {/* Mining & Faucet */}
            <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
                <MiningPanel blockHeight={status.block_height} />
                <FaucetPanel disabled={!status.is_ready} />
            </div>

            {/* Services Grid */}
            <div>
                <h2 className="text-lg font-semibold text-white mb-5">Services</h2>
                <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-5">
                    {status.services.map((service) => (
                        <ServiceCard key={service.name} service={service} />
                    ))}
                </div>
            </div>
        </div>
    );
}

export default Dashboard;
