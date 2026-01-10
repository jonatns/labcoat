import { useEffect, useMemo } from 'react';
import { useBinaries } from '../hooks/useStatus';
import { useStore } from '../lib/store';

export function SetupScreen() {
    const { binaries, downloadBinaries, checkBinaries } = useBinaries();
    const { downloadProgress, isLoading } = useStore();

    useEffect(() => {
        checkBinaries();
    }, []);

    const missingBinaries = useMemo(() => {
        return binaries.filter((b) => b.status === 'notinstalled');
    }, [binaries]);

    const handleInstall = () => {
        downloadBinaries();
    };

    return (
        <div className="flex flex-col items-center justify-center h-screen bg-zinc-950 text-white p-8">
            <div className="max-w-md w-full space-y-8">
                <div className="text-center">
                    <div className="w-20 h-20 bg-indigo-500/10 rounded-3xl flex items-center justify-center mx-auto mb-6">
                        <svg className="w-10 h-10 text-indigo-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                        </svg>
                    </div>
                    <h1 className="text-3xl font-bold tracking-tight mb-2">Setup Isomer</h1>
                    <p className="text-zinc-400 text-lg">
                        We need to download some required components before we can start.
                    </p>
                </div>

                <div className="bg-zinc-900/50 rounded-2xl p-6 border border-zinc-800/50 backdrop-blur-sm">
                    <h3 className="text-sm font-medium text-zinc-400 uppercase tracking-wider mb-4">
                        Required Components
                    </h3>
                    <ul className="space-y-3">
                        {missingBinaries.map((binary) => (
                            <li key={binary.service} className="flex items-center justify-between p-3 bg-zinc-900 rounded-xl border border-zinc-800">
                                <div className="flex items-center gap-3">
                                    <div className="w-8 h-8 rounded-lg bg-zinc-800 flex items-center justify-center">
                                        <span className="text-xs font-mono font-bold text-zinc-400">
                                            {binary.service.charAt(0).toUpperCase()}
                                        </span>
                                    </div>
                                    <div>
                                        <span className="font-medium block">{binary.service}</span>
                                        <span className="text-xs text-zinc-500">
                                            {downloadProgress[binary.service]
                                                ? `${(downloadProgress[binary.service] * 100).toFixed(0)}%`
                                                : 'Pending'}
                                        </span>
                                    </div>
                                </div>
                                {downloadProgress[binary.service] ? (
                                    <div className="w-24 h-1.5 bg-zinc-800 rounded-full overflow-hidden">
                                        <div
                                            className="h-full bg-indigo-500 transition-all duration-300"
                                            style={{ width: `${downloadProgress[binary.service] * 100}%` }}
                                        />
                                    </div>
                                ) : (
                                    <div className="w-2 h-2 rounded-full bg-amber-500/50 animate-pulse" />
                                )}
                            </li>
                        ))}
                    </ul>
                </div>

                <button
                    onClick={handleInstall}
                    disabled={isLoading}
                    className={`
            w-full py-4 px-6 rounded-xl font-semibold text-lg transition-all duration-200
            flex items-center justify-center gap-2
            ${isLoading
                            ? 'bg-zinc-800 text-zinc-500 cursor-not-allowed'
                            : 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-500/20 hover:shadow-indigo-500/30 hover:-translate-y-0.5 active:translate-y-0'}
          `}
                >
                    {isLoading ? (
                        <>
                            <svg className="animate-spin h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
                                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                            </svg>
                            Downloading...
                        </>
                    ) : (
                        'Install Components'
                    )}
                </button>
            </div>
        </div>
    );
}
