import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

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

            await invoke('reset_chain');

            setStatus('Chain reset complete! Services are stopped.');
            setTimeout(() => setStatus(null), 5000);
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
        <div className="p-8 max-w-4xl mx-auto relative">
            {/* Reset Confirmation Modal */}
            {showResetConfirm && (
                <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/80 backdrop-blur-sm">
                    <div className="bg-zinc-900 border border-red-500/30 rounded-xl p-6 max-w-md w-full shadow-2xl">
                        <h3 className="text-xl font-bold text-white mb-2">⚠ WARNING: Destructive Action</h3>
                        <p className="text-zinc-400 mb-4 text-sm">
                            This will <strong>permanently delete</strong> all Isomer data, including:
                            <ul className="list-disc list-inside mt-2 space-y-1 ml-2 text-zinc-500">
                                <li>Bitcoin Core blockchain data</li>
                                <li>Wallet transaction history</li>
                                <li>Metashrew & Ord indexer databases</li>
                            </ul>
                        </p>
                        <p className="text-zinc-300 text-sm mb-2">
                            Type <span className="font-mono font-bold text-red-400">reset</span> below to confirm:
                        </p>
                        <input
                            type="text"
                            value={resetInput}
                            onChange={(e) => setResetInput(e.target.value)}
                            className="w-full bg-black/50 border border-zinc-700 rounded-lg px-3 py-2 text-white focus:outline-none focus:border-red-500 mb-6"
                            placeholder="Type 'reset'"
                            autoFocus
                            autoComplete="off"
                            autoCorrect="off"
                            autoCapitalize="off"
                            spellCheck="false"
                        />
                        <div className="flex gap-3 justify-end">
                            <button
                                onClick={() => {
                                    setShowResetConfirm(false);
                                    setResetInput('');
                                }}
                                className="px-4 py-2 rounded-lg text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors"
                            >
                                Cancel
                            </button>
                            <button
                                onClick={handleReset}
                                disabled={resetInput !== 'reset'}
                                className={`px-4 py-2 rounded-lg font-medium transition-colors ${resetInput === 'reset'
                                    ? 'bg-red-600 hover:bg-red-500 text-white'
                                    : 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
                                    }`}
                            >
                                Confirm Reset
                            </button>
                        </div>
                    </div>
                </div>
            )}

            <header className="mb-8">
                <h1 className="text-3xl font-bold text-white mb-2">Settings</h1>
                <p className="text-zinc-400">Configure your Isomer environment</p>
            </header>

            <div className="space-y-6">
                {/* Binaries Management */}
                <section className="bg-zinc-900/50 rounded-xl border border-zinc-800 overflow-hidden">
                    <div className="p-6 border-b border-zinc-800 bg-zinc-900/10">
                        <h2 className="text-lg font-semibold text-white">Binary Management</h2>
                        <p className="text-sm text-zinc-400 mt-1">Manage external service binaries</p>
                    </div>

                    <div className="p-6">
                        <div className="flex items-center justify-between">
                            <div>
                                <h3 className="text-white font-medium">Service Binaries</h3>
                                <p className="text-sm text-zinc-400 mt-1">
                                    {missingBinaries
                                        ? 'Required binaries are missing.'
                                        : updateAvailable
                                            ? 'Updates are available for some services.'
                                            : 'All binaries are up to date.'}
                                </p>
                            </div>

                            <button
                                onClick={handleDownload}
                                disabled={isDownloading || (!missingBinaries && !updateAvailable)}
                                className={`px-4 py-2 rounded-lg font-medium transition-colors ${isDownloading || (!missingBinaries && !updateAvailable)
                                    ? 'bg-zinc-800 text-zinc-500 cursor-not-allowed'
                                    : 'bg-indigo-600 hover:bg-indigo-500 text-white'
                                    }`}
                            >
                                {isDownloading
                                    ? 'Downloading...'
                                    : missingBinaries
                                        ? 'Download Missing'
                                        : updateAvailable
                                            ? 'Update Binaries'
                                            : 'Check for Updates'}
                            </button>
                        </div>
                    </div>
                </section>

                {/* Danger Zone */}
                <section className="bg-zinc-900/50 rounded-xl border border-red-900/30 overflow-hidden">
                    <div className="p-6 border-b border-red-900/30 bg-red-900/10">
                        <h2 className="text-lg font-semibold text-red-400">Danger Zone</h2>
                        <p className="text-sm text-red-300/70 mt-1">Destructive actions that cannot be undone</p>
                    </div>

                    <div className="p-6">
                        <div className="flex items-center justify-between">
                            <div>
                                <h3 className="text-white font-medium">Reset Chain Data</h3>
                                <p className="text-sm text-zinc-400 mt-1">
                                    Deletes all block data, wallet history, and indexer state.
                                    Services will be stopped and data directories cleared.
                                </p>
                            </div>

                            <button
                                onClick={() => setShowResetConfirm(true)}
                                disabled={isResetting}
                                className={`px-4 py-2 rounded-lg font-medium transition-colors ${isResetting
                                    ? 'bg-zinc-800 text-zinc-500 cursor-not-allowed'
                                    : 'bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/50'
                                    }`}
                            >
                                {isResetting ? 'Resetting...' : 'Reset Chain'}
                            </button>
                        </div>

                        {status && (
                            <div className={`mt-4 p-3 rounded-lg text-sm ${status.startsWith('Error')
                                ? 'bg-red-900/20 text-red-300'
                                : 'bg-green-900/20 text-green-300'
                                }`}>
                                {status}
                            </div>
                        )}
                    </div>
                </section>
            </div>
        </div>
    );
}
