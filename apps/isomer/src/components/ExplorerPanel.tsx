import { ExternalLink } from 'lucide-react';

export function ExplorerPanel() {
    const espoUrl = 'http://localhost:8081';

    return (
        <div className="flex flex-col h-full bg-zinc-950 relative">
            <div className="flex-1 w-full relative">
                <iframe
                    src={espoUrl}
                    className="absolute inset-0 w-full h-full border-none"
                    title="Espo Explorer"
                />
                {/* Open in New Tab button */}
                <a
                    href={espoUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="absolute top-4 right-4 z-10 p-2.5 bg-zinc-900/60 backdrop-blur-md hover:bg-zinc-800/80 rounded-lg transition-all duration-200 border border-zinc-700/50 hover:border-zinc-600 group"
                    title="Open in New Tab"
                >
                    <ExternalLink size={18} className="text-zinc-400 group-hover:text-white transition-colors" />
                </a>
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
