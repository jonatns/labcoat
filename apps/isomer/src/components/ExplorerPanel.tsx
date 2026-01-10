
export function ExplorerPanel() {
    const espoUrl = 'http://localhost:8081';

    return (
        <div className="flex flex-col h-full bg-zinc-950">
            <div className="flex items-center justify-between p-4 border-b border-zinc-800 bg-zinc-900/50">
                <div>
                    <h2 className="text-xl font-bold text-white">Espo Explorer</h2>
                    <p className="text-sm text-zinc-500">View blocks, transactions, and Alkanes</p>
                </div>
                <div className="flex items-center gap-3">
                    <a
                        href={espoUrl}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="px-3 py-1.5 text-xs font-medium text-zinc-400 hover:text-white bg-zinc-800 hover:bg-zinc-700 rounded-md transition-colors border border-zinc-700"
                    >
                        Open in New Tab
                    </a>
                </div>
            </div>

            <div className="flex-1 w-full relative">
                <iframe
                    src={espoUrl}
                    className="absolute inset-0 w-full h-full border-none"
                    title="Espo Explorer"
                />
                {/* Simple overlay if the iframe fails to load or for initial state */}
                <div className="absolute inset-0 flex items-center justify-center -z-10 bg-zinc-900">
                    <div className="text-center">
                        <div className="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto mb-4"></div>
                        <p className="text-zinc-500">Connecting to Espo...</p>
                    </div>
                </div>
            </div>
        </div>
    );
}

export default ExplorerPanel;
