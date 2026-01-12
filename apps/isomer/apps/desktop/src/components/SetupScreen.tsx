import { useEffect, useMemo } from 'react';
import { useBinaries } from '../hooks/useStatus';
import { useStore } from '../lib/store';
import { Check } from 'lucide-react';

// Estimated sizes in MB for each component
const COMPONENT_SIZES: Record<string, number> = {
    'Bitcoin Core': 45,
    'Metashrew (Indexer)': 25,
    'Ord (Inscriptions)': 15,
    'Esplora (Explorer)': 15,
    'Espo (Indexer/Explorer)': 30,
    'JSON RPC': 10,
};

// Display name mapping
const DISPLAY_NAMES: Record<string, string> = {
    'Bitcoin Core': 'Bitcoin Core',
    'Metashrew (Indexer)': 'Metashrew',
    'Ord (Inscriptions)': 'Ord',
    'Esplora (Explorer)': 'Esplora',
    'Espo (Indexer/Explorer)': 'Espo',
    'JSON RPC': 'RPC',
};

export function SetupScreen() {
    const { binaries, downloadBinaries, checkBinaries } = useBinaries();
    const { downloadProgress, isLoading } = useStore();

    useEffect(() => {
        checkBinaries();
    }, []);

    const missingBinaries = useMemo(() => {
        return binaries.filter((b) => b.status === 'notinstalled');
    }, [binaries]);

    // Calculate total download size
    const totalSize = useMemo(() => {
        return missingBinaries.reduce((acc, b) => {
            return acc + (COMPONENT_SIZES[b.service] || 10);
        }, 0);
    }, [missingBinaries]);

    const handleInstall = () => {
        downloadBinaries();
    };

    // Check if a download is complete (progress === 1)
    const isComplete = (service: string) => downloadProgress[service] === 1;

    return (
        <div className="flex flex-col items-center justify-center h-screen bg-zinc-950 text-white p-6">
            <div className="max-w-md w-full space-y-4">
                <div className="text-center">
                    <div className="w-14 h-14 bg-indigo-500/10 rounded-2xl flex items-center justify-center mx-auto mb-4">
                        <svg className="w-7 h-7 text-indigo-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                        </svg>
                    </div>
                    <h1 className="text-2xl font-bold tracking-tight mb-1">Setup Isomer</h1>
                    <p className="text-zinc-400 text-sm">
                        Download required components to get started.
                    </p>
                </div>

                <div className="bg-zinc-900/50 rounded-xl p-4 border border-zinc-800/50 backdrop-blur-sm">
                    <div className="flex items-center justify-between mb-3">
                        <h3 className="text-xs font-medium text-zinc-400 uppercase tracking-wider">
                            Components
                        </h3>
                        <span className="text-xs text-zinc-500">
                            ~{totalSize}MB
                        </span>
                    </div>
                    <ul className="space-y-1.5">
                        {missingBinaries.map((binary) => {
                            const name = DISPLAY_NAMES[binary.service] || binary.service;
                            const size = COMPONENT_SIZES[binary.service] || 10;
                            const progress = downloadProgress[binary.service];
                            const complete = isComplete(binary.service);

                            return (
                                <li key={binary.service} className="flex items-center justify-between py-2 px-3 bg-zinc-900 rounded-lg border border-zinc-800/50">
                                    <div className="flex items-center gap-2.5 min-w-0">
                                        <div className={`w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0 transition-colors ${complete ? 'bg-green-500/20' : 'bg-zinc-800'}`}>
                                            {complete ? (
                                                <Check size={12} className="text-green-500" />
                                            ) : (
                                                <span className="text-[10px] font-mono font-bold text-zinc-400">
                                                    {name.charAt(0).toUpperCase()}
                                                </span>
                                            )}
                                        </div>
                                        <span className={`text-sm font-medium truncate ${complete ? 'text-green-400' : ''}`}>{name}</span>
                                    </div>
                                    <div className="flex items-center gap-2 flex-shrink-0 ml-2">
                                        <span className="text-xs text-zinc-500">
                                            {complete ? '✓' : progress ? `${(progress * 100).toFixed(0)}%` : `${size}MB`}
                                        </span>
                                        {!complete && progress && (
                                            <div className="w-12 h-1 bg-zinc-800 rounded-full overflow-hidden">
                                                <div
                                                    className="h-full bg-indigo-500 transition-all duration-300"
                                                    style={{ width: `${progress * 100}%` }}
                                                />
                                            </div>
                                        )}
                                    </div>
                                </li>
                            )
                        })}
                    </ul>
                </div>

                <button
                    onClick={handleInstall}
                    disabled={isLoading}
                    className={`
                        w-full py-3 px-6 rounded-xl font-semibold transition-all duration-200
                        flex items-center justify-center gap-2
                        ${isLoading
                            ? 'bg-zinc-800 text-zinc-500 cursor-not-allowed'
                            : 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-500/20 hover:shadow-indigo-500/30 hover:-translate-y-0.5 active:translate-y-0'}
                    `}
                >
                    {isLoading ? (
                        <>
                            <svg className="animate-spin h-4 w-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                            </svg>
                            <span>Downloading...</span>
                        </>
                    ) : (
                        `Install (~${totalSize}MB)`
                    )}
                </button>
            </div>
        </div>
    );
}
