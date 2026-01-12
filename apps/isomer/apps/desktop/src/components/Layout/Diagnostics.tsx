import { Terminal, Search, ChevronUp, ChevronDown, Maximize2, Minimize2 } from 'lucide-react';
import { useState, useEffect } from 'react';
import { LogsPanel } from '../LogsPanel'; // Reuse existing logs panel logic for now
import { ExplorerPanel } from '../ExplorerPanel'; // Reuse explorer logic

type Tab = 'logs' | 'explorer';

interface DiagnosticsProps {
    services: any[]; // Using any[] to match the flexible service type for now
}

export function Diagnostics({ services }: DiagnosticsProps) {
    const [activeTab, setActiveTab] = useState<Tab>('logs');
    const [isExpanded, setIsExpanded] = useState(false); // Default to collapsed
    const [isMaximized, setIsMaximized] = useState(false);

    // Auto-expand on error
    useEffect(() => {
        if (services.some(s => s.status === 'error')) {
            setIsExpanded(true);
            setActiveTab('logs');
        }
    }, [services]);

    const hasError = services.some(s => s.status === 'error');

    return (
        <div className={`
      flex flex-col border-t border-zinc-800 bg-zinc-950 transition-all duration-300 ease-[cubic-bezier(0.2,0,0,1)]
      ${isMaximized ? 'h-[85vh]' : isExpanded ? 'h-72' : 'h-10'}
    `}>

            {/* Tab Bar / Header */}
            <div
                className="flex items-center justify-between px-4 h-10 bg-zinc-900/50 cursor-pointer hover:bg-zinc-900 transition-colors select-none border-b border-transparent"
                onClick={() => {
                    // clicking header toggles expansion, unless maximized (then it does nothing or minimizes?)
                    if (isMaximized) return;
                    setIsExpanded(!isExpanded);
                }}
            >
                <div className="flex items-center gap-1">
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('logs'); setIsExpanded(true); }}
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors relative
               ${activeTab === 'logs' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Terminal className="w-3.5 h-3.5" />
                        Live Logs
                        {hasError && <span className="absolute top-2 right-1 w-1.5 h-1.5 rounded-full bg-rose-500 animate-pulse" />}
                    </button>
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('explorer'); setIsExpanded(true); }}
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors
               ${activeTab === 'explorer' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Search className="w-3.5 h-3.5" />
                        Explorer
                    </button>
                </div>

                <div className="flex items-center gap-4">
                    <span className="text-[10px] text-zinc-600 font-mono uppercase tracking-wider hidden sm:block">
                        Diagnostics Layer
                    </span>

                    <div className="flex items-center gap-1">
                        {/* Maximize Toggle */}
                        {isExpanded && (
                            <button
                                onClick={(e) => {
                                    e.stopPropagation();
                                    setIsMaximized(!isMaximized);
                                }}
                                className="p-1.5 hover:bg-zinc-800 rounded text-zinc-500 hover:text-zinc-300 transition-colors"
                            >
                                {isMaximized ? <Minimize2 className="w-3.5 h-3.5" /> : <Maximize2 className="w-3.5 h-3.5" />}
                            </button>
                        )}

                        {/* Expand/Collapse Chevron */}
                        <button
                            className="p-1.5 hover:bg-zinc-800 rounded text-zinc-500 hover:text-zinc-300 transition-colors"
                            onClick={(e) => {
                                e.stopPropagation();
                                if (isMaximized) setIsMaximized(false);
                                else setIsExpanded(!isExpanded);
                            }}
                        >
                            {isExpanded ? <ChevronDown className="w-4 h-4" /> : <ChevronUp className="w-4 h-4" />}
                        </button>
                    </div>
                </div>
            </div>

            {/* Content Area */}
            {isExpanded && (
                <div className="flex-1 overflow-hidden relative">
                    {activeTab === 'logs' ? (
                        <div className="h-full w-full">
                            {/* We wrap the existing LogsPanel but override styling via CSS or assume it fits */}
                            <LogsPanel />
                        </div>
                    ) : (
                        <div className="h-full w-full">
                            <ExplorerPanel />
                        </div>
                    )}
                </div>
            )}
        </div>
    );
}
