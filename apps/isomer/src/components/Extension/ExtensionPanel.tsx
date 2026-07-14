import React, { useState } from 'react';
import { X, Copy, Check, Hexagon, Download, Loader2 } from 'lucide-react';

interface ExtensionPanelProps {
    isOpen: boolean;
    onClose: () => void;
    extensionPath: string | null;
    onDownload: () => Promise<void>;
    isDownloading: boolean;
}

export const ExtensionPanel: React.FC<ExtensionPanelProps> = ({
    isOpen,
    onClose,
    extensionPath,
    onDownload,
    isDownloading
}) => {
    const [copied, setCopied] = useState(false);

    const handleCopyPath = async () => {
        if (!extensionPath) return;
        await navigator.clipboard.writeText(extensionPath);
        setCopied(true);
        setTimeout(() => setCopied(false), 2000);
    };

    if (!isOpen) return null;

    const isInstalled = !!extensionPath;

    return (
        <>
            {/* Backdrop */}
            <div
                className="fixed inset-0 bg-black/50 backdrop-blur-sm z-40 transition-opacity"
                onClick={onClose}
            />

            {/* Slide-out Panel */}
            <div className={`
                fixed right-0 top-0 h-full w-[420px] bg-zinc-950 border-l border-zinc-800 z-50
                shadow-2xl shadow-black transform transition-transform duration-300 ease-out
                ${isOpen ? 'translate-x-0' : 'translate-x-full'}
            `}>
                {/* Header */}
                <div className="flex items-center justify-between p-6 border-b border-zinc-900">
                    <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center justify-center">
                            <Hexagon className="w-5 h-5 text-blue-500" />
                        </div>
                        <div>
                            <h2 className="text-lg font-semibold text-zinc-100">Companion Service</h2>
                            <p className="text-xs text-zinc-500 font-mono uppercase tracking-wider">Browser Bridge</p>
                        </div>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-2 rounded-lg hover:bg-zinc-900 text-zinc-500 hover:text-white transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* Content */}
                <div className="p-6 space-y-8">

                    {!isInstalled ? (
                        /* Not Installed State */
                        <div className="space-y-6">
                            <div className="p-4 rounded-xl bg-blue-500/5 border border-blue-500/10 space-y-3">
                                <h3 className="text-sm font-medium text-blue-400">Setup Required</h3>
                                <p className="text-xs text-zinc-400 leading-relaxed">
                                    The browser bridge is required for application simulation.
                                    Please retrieve the binary bundle to proceed.
                                </p>
                            </div>

                            <button
                                onClick={onDownload}
                                disabled={isDownloading}
                                className={`
                                    w-full flex items-center justify-center gap-2 py-3 px-4 rounded-lg
                                    font-medium transition-all duration-200 border
                                    ${isDownloading
                                        ? 'bg-zinc-900 border-zinc-800 text-zinc-500 cursor-wait'
                                        : 'bg-zinc-100 text-zinc-900 border-transparent hover:bg-white'}
                                `}
                            >
                                {isDownloading ? (
                                    <>
                                        <Loader2 className="w-4 h-4 animate-spin" />
                                        Retrieving Bundle...
                                    </>
                                ) : (
                                    <>
                                        <Download className="w-4 h-4" />
                                        Retrieve Bundle
                                    </>
                                )}
                            </button>
                        </div>
                    ) : (
                        /* Installed State */
                        <>
                            {/* Status */}
                            <div className="flex items-center gap-3 p-4 rounded-xl bg-emerald-500/5 border border-emerald-500/10">
                                <div className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
                                <span className="text-sm text-emerald-500 font-medium">Service Active</span>
                            </div>

                            {/* Extension Path */}
                            <div className="space-y-2">
                                <label className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest">
                                    Local Path
                                </label>
                                <div className="flex items-center gap-2 p-3 rounded-lg bg-zinc-900 border border-zinc-800 group hover:border-zinc-700 transition-colors">
                                    <code className="flex-1 text-xs text-zinc-300 font-mono truncate select-all">
                                        {extensionPath}
                                    </code>
                                    <button
                                        onClick={handleCopyPath}
                                        className={`
                                            p-2 rounded-md transition-all
                                            ${copied
                                                ? 'bg-emerald-500/10 text-emerald-500'
                                                : 'hover:bg-zinc-800 text-zinc-500 hover:text-white'}
                                        `}
                                        title="Copy path"
                                    >
                                        {copied ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                                    </button>
                                </div>
                            </div>

                            {/* Installation Steps */}
                            <div className="space-y-4">
                                <label className="text-[10px] font-bold text-zinc-500 uppercase tracking-widest">
                                    Injection Steps
                                </label>

                                <div className="space-y-2">
                                    <Step number={1} title="Open Extensions">
                                        Navigate to <code className="text-blue-400">chrome://extensions</code>
                                    </Step>

                                    <Step number={2} title="Developer Mode">
                                        Enable <strong className="text-zinc-300">Developer mode</strong> (top-right)
                                    </Step>

                                    <Step number={3} title="Load Component">
                                        Click <strong className="text-zinc-300">Load unpacked</strong> and select the path
                                    </Step>
                                </div>
                            </div>

                            {/* Quick Copy Button */}
                            <button
                                onClick={handleCopyPath}
                                className={`
                                    w-full flex items-center justify-center gap-2 py-3 px-4 rounded-lg
                                    font-medium transition-all duration-200 text-sm
                                    ${copied
                                        ? 'bg-emerald-500/10 text-emerald-500 border border-emerald-500/20'
                                        : 'bg-zinc-900 text-zinc-300 border border-zinc-800 hover:border-zinc-700 hover:text-white'}
                                `}
                            >
                                {copied ? (
                                    <>
                                        <Check className="w-4 h-4" />
                                        Path Copied
                                    </>
                                ) : (
                                    <>
                                        <Copy className="w-4 h-4" />
                                        Copy Path
                                    </>
                                )}
                            </button>
                        </>
                    )}
                </div>
            </div>
        </>
    );
};

interface StepProps {
    number: number;
    title: string;
    children: React.ReactNode;
}

const Step: React.FC<StepProps> = ({ number, title, children }) => (
    <div className="flex gap-3 p-3 rounded-lg bg-zinc-900/50 border border-zinc-800/50">
        <div className="flex-shrink-0 w-6 h-6 rounded-md bg-zinc-800 flex items-center justify-center text-xs font-bold text-zinc-400 font-mono">
            {number}
        </div>
        <div className="flex-1 min-w-0">
            <p className="text-sm font-medium text-zinc-200">{title}</p>
            <p className="text-xs text-zinc-500 mt-0.5 font-mono">{children}</p>
        </div>
    </div>
);
