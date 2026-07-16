import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Download, RefreshCw, Trash2, AlertTriangle, HardDrive, ShieldAlert } from 'lucide-react';
import { useBinaries } from '../hooks/useStatus';

export function SettingsPanel() {
    const [isResetting, setIsResetting] = useState(false);
    const [status, setStatus] = useState<string | null>(null);
    const { binaries, checkBinaries, downloadBinaries } = useBinaries();
    const [isDownloading, setIsDownloading] = useState(false);

    // Check binaries on mount
    useState(() => {
        checkBinaries();
    });

    const handleDownload = async () => {
        setIsDownloading(true);
        try {
            await downloadBinaries();
        } catch (err) {
            console.error('Failed to download binaries:', err);
        } finally {
            setIsDownloading(false);
        }
    };

    const [showResetConfirm, setShowResetConfirm] = useState(false);
    const [resetInput, setResetInput] = useState('');

    const handleReset = async () => {
        setIsResetting(true);
        setStatus('Stopping services and clearing data...');
        setShowResetConfirm(false); // Close modal
        setResetInput('');

        try {
            await invoke('reset_chain');

            // Find current status
            setTimeout(() => setStatus(null), 5000);
            setStatus('Chain reset complete! Services are stopped.');
        } catch (error) {
            console.error('Failed to reset chain:', error);
            setStatus(`Error: ${error}`);
        } finally {
            setIsResetting(false);
        }
    };

    const missingBinaries = binaries.some(b => b.status === 'notinstalled');
    const updateAvailable = binaries.some(b => typeof b.status === 'object' && 'updateavailable' in b.status);

    return (
        <div className="min-h-full bg-zinc-950 p-8">
            <div className="max-w-4xl mx-auto space-y-10">
                {/* Reset Confirmation Modal */}
                {showResetConfirm && (
                    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm animate-in fade-in duration-200">
                        <div className="bg-zinc-900 border border-red-500/30 rounded-xl p-6 max-w-md w-full shadow-2xl">
                            <div className="flex items-center gap-3 mb-4 text-red-400">
                                <AlertTriangle className="w-6 h-6" />
                                <h3 className="text-xl font-bold text-white">Destructive Action</h3>
                            </div>
                            <p className="text-zinc-400 mb-4 text-sm leading-relaxed">
                                This will <strong>permanently delete</strong> all Isomer data, including:
                            </p>
                            <ul className="list-disc list-inside mb-6 space-y-1 ml-2 text-zinc-500 text-sm">
                                <li>Bitcoin Core blockchain data</li>
                                <li>Wallet transaction history</li>
                                <li>Metashrew & Ord indexer databases</li>
                            </ul>

                            <p className="text-zinc-300 text-sm mb-2">
                                Type <span className="font-mono font-bold text-red-400">reset</span> below to confirm:
                            </p>
                            <input
                                type="text"
                                value={resetInput}
                                onChange={(e) => setResetInput(e.target.value)}
                                className="w-full bg-black/50 border border-zinc-700 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-red-500 mb-6 font-mono"
                                placeholder="reset"
                                autoFocus
                                autoComplete="off"
                            />
                            <div className="flex gap-3 justify-end">
                                <button
                                    onClick={() => {
                                        setShowResetConfirm(false);
                                        setResetInput('');
                                    }}
                                    className="px-4 py-2 rounded-lg text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors text-sm font-medium"
                                >
                                    Cancel
                                </button>
                                <button
                                    onClick={handleReset}
                                    disabled={resetInput !== 'reset'}
                                    className={`px-4 py-2 rounded-lg text-sm font-bold transition-colors ${resetInput === 'reset'
                                        ? 'bg-red-600 hover:bg-red-500 text-white shadow-lg shadow-red-900/20'
                                        : 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
                                        }`}
                                >
                                    Confirm Reset
                                </button>
                            </div>
                        </div>
                    </div>
                )}

                <header className="border-b border-zinc-900 pb-6">
                    <h1 className="text-2xl font-bold text-white tracking-tight mb-2">Settings</h1>
                    <p className="text-zinc-500 text-sm">Configure your Isomer environment and data</p>
                </header>

                <div className="space-y-8">
                    {/* Binaries Management */}
                    <section>
                        <div className="text-[10px] uppercase tracking-wider text-zinc-500 font-bold mb-4 flex items-center gap-2">
                            <HardDrive className="w-3.5 h-3.5" />
                            System Binaries
                        </div>

                        <div className="bg-zinc-900/30 rounded-lg border border-zinc-900 overflow-hidden">
                            <div className="p-6 flex items-center justify-between">
                                <div>
                                    <h3 className="text-zinc-200 font-medium">Core Services</h3>
                                    <p className="text-sm text-zinc-500 mt-1">
                                        {missingBinaries
                                            ? 'Required binaries are missing.'
                                            : updateAvailable
                                                ? 'Updates are available for some services.'
                                                : 'All binaries are up to date and verified.'}
                                    </p>
                                </div>

                                <button
                                    onClick={handleDownload}
                                    disabled={isDownloading || (!missingBinaries && !updateAvailable)}
                                    className={`
                                        px-5 py-2.5 rounded-md text-sm font-semibold shadow-lg transition-all duration-200 flex items-center gap-2
                                        ${isDownloading || (!missingBinaries && !updateAvailable)
                                            ? 'bg-zinc-800 text-zinc-600 cursor-not-allowed shadow-none'
                                            : 'bg-zinc-100 text-zinc-950 hover:bg-white hover:scale-[1.02] shadow-indigo-500/10'
                                        }
                                    `}
                                >
                                    {isDownloading ? (
                                        <RefreshCw className="w-4 h-4 animate-spin text-indigo-600" />
                                    ) : (
                                        <Download className="w-4 h-4" />
                                    )}
                                    {isDownloading
                                        ? 'Downloading...'
                                        : missingBinaries
                                            ? 'Download Missing'
                                            : updateAvailable
                                                ? 'Update All'
                                                : 'Check for Updates'}
                                </button>
                            </div>
                        </div>
                    </section>

                    {/* Danger Zone */}
                    <section>
                        <div className="text-[10px] uppercase tracking-wider text-red-900/70 font-bold mb-4 flex items-center gap-2">
                            <ShieldAlert className="w-3.5 h-3.5" />
                            Danger Zone
                        </div>

                        <div className="bg-red-950/10 rounded-lg border border-red-900/20 overflow-hidden">
                            <div className="p-6 flex items-center justify-between">
                                <div>
                                    <h3 className="text-red-400 font-medium">Reset Chain Data</h3>
                                    <p className="text-sm text-red-900/60 mt-1 max-w-md">
                                        Permanently deletes all block data, wallet history, and indexer state.
                                        Services will be stopped immediately.
                                    </p>
                                </div>

                                <button
                                    onClick={() => setShowResetConfirm(true)}
                                    disabled={isResetting}
                                    className={`
                                        px-5 py-2.5 rounded-md text-sm font-semibold transition-all duration-200 flex items-center gap-2 border
                                        ${isResetting
                                            ? 'bg-zinc-800 text-zinc-500 border-transparent cursor-not-allowed'
                                            : 'bg-red-500/10 text-red-500 border-red-500/20 hover:bg-red-500/20 hover:border-red-500/30'
                                        }
                                    `}
                                >
                                    <Trash2 className="w-4 h-4" />
                                    {isResetting ? 'Resetting...' : 'Reset Chain'}
                                </button>
                            </div>

                            {status && (
                                <div className={`px-6 pb-6 text-sm flex items-center gap-2 ${status.startsWith('Error') ? 'text-red-400' : 'text-green-500'}`}>
                                    <div className={`w-1.5 h-1.5 rounded-full ${status.startsWith('Error') ? 'bg-red-500' : 'bg-green-500'}`} />
                                    {status}
                                </div>
                            )}
                        </div>
                    </section>
                </div>
            </div>
        </div>
    );
}
