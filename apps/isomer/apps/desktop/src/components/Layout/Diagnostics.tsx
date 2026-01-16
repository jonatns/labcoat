import { Terminal, Search, ChevronUp, ChevronDown, Maximize2, Minimize2, Wallet, Settings } from 'lucide-react';
import { useState, useEffect, useRef } from 'react';
import { LogsPanel } from '../LogsPanel';
import { ExplorerPanel } from '../ExplorerPanel';
import { WalletsPanel } from '../Wallet/WalletsPanel';
import { SettingsPanel } from '../SettingsPanel';

type Tab = 'logs' | 'explorer' | 'wallets' | 'settings';

interface DiagnosticsProps {
    services: any[];
    isSystemRunning?: boolean;
}

export function Diagnostics({ services, isSystemRunning = false }: DiagnosticsProps) {
    const [activeTab, setActiveTab] = useState<Tab>('logs');
    const [isExpanded, setIsExpanded] = useState(false); // Default to collapsed
    const [isMaximized, setIsMaximized] = useState(false);

    // Track if we've already auto-expanded for the current error state
    const hasAutoExpandedRef = useRef(false);

    // Auto-expand on error (only once per error occurrence)
    useEffect(() => {
        const hasError = services.some(s => s.status === 'error');

        if (hasError && !hasAutoExpandedRef.current) {
            // First time seeing an error, auto-expand to logs
            setIsExpanded(true);
            setActiveTab('logs');
            hasAutoExpandedRef.current = true;
        } else if (!hasError) {
            // No errors, reset the flag so we can auto-expand again if a new error occurs
            hasAutoExpandedRef.current = false;
        }
    }, [services]);

    // Keyboard shortcuts
    useEffect(() => {
        const handleKeyDown = (e: KeyboardEvent) => {
            // Don't capture if user is typing in an input
            if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

            // E opens explorer
            if (e.key === 'e' || e.key === 'E') {
                e.preventDefault();
                setActiveTab('explorer');
                setIsExpanded(true);
            }

            // C closes the panel
            if ((e.key === 'c' || e.key === 'C') && isExpanded) {
                e.preventDefault();
                setIsExpanded(false);
                setIsMaximized(false);
            }

            if (e.key === 'Escape') {
                if (isMaximized) {
                    setIsMaximized(false);
                } else if (isExpanded) {
                    setIsExpanded(false);
                }
            }

            if ((e.key === 'm' || e.key === 'M') && isExpanded && !e.metaKey && !e.ctrlKey) {
                e.preventDefault();
                setIsMaximized(prev => !prev);
            }
        };

        window.addEventListener('keydown', handleKeyDown);
        return () => window.removeEventListener('keydown', handleKeyDown);
    }, [isExpanded, isMaximized]);

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
                <div className="flex items-center gap-1" role="tablist" aria-label="Diagnostics panels">
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('logs'); setIsExpanded(true); }}
                        role="tab"
                        aria-selected={activeTab === 'logs'}
                        aria-controls="diagnostics-logs-panel"
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors relative
               ${activeTab === 'logs' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Terminal className="w-3.5 h-3.5" aria-hidden="true" />
                        Live Logs
                        {hasError && <span className="absolute top-2 right-1 w-1.5 h-1.5 rounded-full bg-rose-500 animate-pulse" aria-label="Error indicator" />}
                    </button>
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('explorer'); setIsExpanded(true); }}
                        role="tab"
                        aria-selected={activeTab === 'explorer'}
                        aria-controls="diagnostics-explorer-panel"
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors
               ${activeTab === 'explorer' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Search className="w-3.5 h-3.5" aria-hidden="true" />
                        Explorer
                    </button>
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('wallets'); setIsExpanded(true); }}
                        role="tab"
                        aria-selected={activeTab === 'wallets'}
                        aria-controls="diagnostics-wallets-panel"
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors
               ${activeTab === 'wallets' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Wallet className="w-3.5 h-3.5" aria-hidden="true" />
                        Wallets
                    </button>
                    <button
                        onClick={(e) => { e.stopPropagation(); setActiveTab('settings'); setIsExpanded(true); }}
                        role="tab"
                        aria-selected={activeTab === 'settings'}
                        aria-controls="diagnostics-settings-panel"
                        className={`
               flex items-center gap-2 px-3 h-10 text-xs font-medium border-b-2 transition-colors
               ${activeTab === 'settings' ? 'border-amber-500 text-zinc-200 bg-zinc-800/20' : 'border-transparent text-zinc-500 hover:text-zinc-300'}
             `}
                    >
                        <Settings className="w-3.5 h-3.5" aria-hidden="true" />
                        Settings
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

            {/* Content Area - always mounted for smooth transitions */}
            <div className={`
                flex-1 overflow-hidden relative transition-all duration-300 ease-out
                ${isExpanded ? 'opacity-100' : 'opacity-0 pointer-events-none'}
            `}>
                {activeTab === 'logs' && (
                    <div className="h-full w-full">
                        <LogsPanel />
                    </div>
                )}
                {activeTab === 'explorer' && (
                    <div className="h-full w-full">
                        <ExplorerPanel
                            onBlockSelect={() => setIsMaximized(true)}
                            isVisible={isExpanded}
                            isMaximized={isMaximized}
                        />
                    </div>
                )}
                {activeTab === 'wallets' && (
                    <div className="h-full w-full overflow-auto">
                        <WalletsPanel isRunning={isSystemRunning} />
                    </div>
                )}
                {activeTab === 'settings' && (
                    <div className="h-full w-full overflow-auto">
                        <SettingsPanel />
                    </div>
                )}
            </div>
        </div>
    );
}
