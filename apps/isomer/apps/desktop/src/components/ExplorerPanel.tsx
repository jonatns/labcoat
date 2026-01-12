import { ExternalLink } from 'lucide-react';
import { ExplorerHome } from './Explorer/ExplorerHome';

export function ExplorerPanel() {
    const openExternalExplorer = () => {
        // Fallback to external or local espo if needed
        window.open('http://localhost:8081', '_blank');
    };

    return (
        <div className="h-full flex flex-col bg-zinc-950">
            {/* Identity Header */}
            <div className="flex items-center justify-between px-4 py-2 bg-zinc-950 border-b border-zinc-900/50 flex-shrink-0 min-h-[40px]">
                <div className="flex items-center gap-2">
                    <span className="text-xs font-medium text-zinc-400 tracking-wide uppercase">Explorer · Native</span>
                    <span className="px-1.5 py-0.5 text-[10px] font-medium bg-indigo-500/10 text-indigo-400 rounded-sm border border-indigo-500/20">
                        RPC
                    </span>
                </div>
                <button
                    onClick={openExternalExplorer}
                    className="flex items-center gap-1.5 text-[10px] font-medium text-zinc-500 hover:text-zinc-300 transition-colors px-2 py-1 hover:bg-zinc-900 rounded"
                >
                    <span>Open in Espo</span>
                    <ExternalLink size={10} />
                </button>
            </div>

            {/* Native Explorer Content */}
            <div className="flex-1 overflow-hidden relative">
                <ExplorerHome />
            </div>
        </div>
    );
}

export default ExplorerPanel;
