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

    const handleReset = async () => {


        setIsResetting(true);
        setStatus('Stopping services and clearing data...');

        try {
            await invoke('reset_chain');

            setStatus('Restarting services...');
            await invoke('start_services');

            setStatus('Chain reset complete! Wallet re-initialized.');
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
        <div className="p-8 max-w-4xl mx-auto">
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
                                    Services will be restarted and a fresh chain will be initialized.
                                </p>
                            </div>

                            <button
                                onClick={() => {
                                    const userInput = prompt('To confirm reset, type "reset" below:\n\nWARNING: This will delete:\n- Bitcoin Core data (blocks/chain)\n- Wallet history\n- Metashrew index\n- Ord index\n- Esplora index');
                                    if (userInput === 'reset') {
                                        handleReset();
                                    }
                                }}
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
