import { ReactNode } from 'react';

interface AppLayoutProps {
    header: ReactNode;
    hero: ReactNode;
    main: ReactNode;
    bottom: ReactNode;
}

export function AppLayout({ header, hero, main, bottom }: AppLayoutProps) {
    return (
        <div className="flex flex-col h-screen w-screen bg-zinc-950 text-zinc-100 overflow-hidden font-sans">
            {/* Fixed Header */}
            <div className="flex-none">
                {header}
            </div>

            {/* Scrollable Middle Area (Hero + Service Grid) */}
            <div className="flex-1 flex flex-col overflow-y-auto min-h-0">
                <div className="flex-none">
                    {hero}
                </div>
                <div className="flex-1">
                    {main}
                </div>
            </div>

            {/* Fixed Bottom Panel */}
            <div className="flex-none z-20 shadow-[0_-5px_20px_rgba(0,0,0,0.5)]">
                {bottom}
            </div>
        </div>
    );
}
