import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export function SettingsPanel() {
    const [isResetting, setIsResetting] = useState(false);
    const [status, setStatus] = useState<string | null>(null);

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

    return (
        <div className="p-8 max-w-4xl mx-auto">
            <header className="mb-8">
                <h1 className="text-3xl font-bold text-white mb-2">Settings</h1>
                <p className="text-zinc-400">Configure your Isomer environment</p>
            </header>

            <div className="space-y-6">
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
