import { useState } from 'react';
import { api } from '../lib/api';

interface MiningPanelProps {
    blockHeight: number;
    onMined?: (newHeight: number) => void;
}

export function MiningPanel({ blockHeight, onMined }: MiningPanelProps) {
    const [count, setCount] = useState<string>('1');
    const [isMining, setIsMining] = useState(false);

    const handleMine = async () => {
        setIsMining(true);
        try {
            const blocksToMine = Math.max(1, parseInt(count) || 1);
            const newHeight = await api.mineBlocks(blocksToMine);
            onMined?.(newHeight);
            // Dont reset input to allow repeated mining
        } catch (error) {
            console.error('Mining failed:', error);
        } finally {
            setIsMining(false);
        }
    };

    return (
        <div className="glass rounded-xl p-6">
            <div className="flex items-center justify-between mb-5">
                <div className="flex items-center gap-2">
                    <span className="text-xl">⛏️</span>
                    <h2 className="text-lg font-semibold text-white">Mining Controls</h2>
                </div>
                <div className="flex items-center gap-2">
                    <span className="text-zinc-500 text-sm">Block Height</span>
                    <span className="font-mono text-xl text-indigo-400">{blockHeight}</span>
                </div>
            </div>

            <div className="flex items-center gap-3">
                <div className="flex-1">
                    <label className="block text-sm text-zinc-400 mb-1">Blocks to mine</label>
                    <input
                        type="number"
                        min={1}
                        max={1000}
                        value={count}
                        onChange={(e) => setCount(e.target.value)}
                        onBlur={() => {
                            // Validate on blur
                            if (!count || parseInt(count) < 1) {
                                setCount('1');
                            } else if (parseInt(count) > 1000) {
                                setCount('1000');
                            }
                        }}
                        className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 
                       text-white font-mono focus:outline-none focus:border-indigo-500"
                    />
                </div>
                <button
                    onClick={handleMine}
                    disabled={isMining}
                    className="mt-6 px-6 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:bg-zinc-700 
                     disabled:cursor-not-allowed rounded-lg font-medium text-white 
                     transition-colors flex items-center gap-2"
                >
                    {isMining ? (
                        <>
                            <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
                                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                            </svg>
                            Mining...
                        </>
                    ) : (
                        <>
                            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 10V3L4 14h7v7l9-11h-7z" />
                            </svg>
                            Mine
                        </>
                    )}
                </button>
            </div>

            <div className="mt-4 flex items-center gap-3">
                <button
                    onClick={() => api.mineBlocks(1)}
                    className="flex-1 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-sm text-zinc-300"
                >
                    +1 Block
                </button>
                <button
                    onClick={() => api.mineBlocks(10)}
                    className="flex-1 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-sm text-zinc-300"
                >
                    +10 Blocks
                </button>
                <button
                    onClick={() => api.mineBlocks(100)}
                    className="flex-1 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 rounded-lg text-sm text-zinc-300"
                >
                    +100 Blocks
                </button>
            </div>
        </div>
    );
}

export default MiningPanel;
