interface NavItem {
    id: string;
    label: string;
    icon: React.ReactNode;
}

interface SidebarProps {
    activeView: string;
    onNavigate: (view: string) => void;
}

const navItems: NavItem[] = [
    {
        id: 'dashboard',
        label: 'Dashboard',
        icon: (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z" />
            </svg>
        ),
    },
    {
        id: 'explorer',
        label: 'Explorer',
        icon: (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
        ),
    },
    {
        id: 'wallets',
        label: 'Wallets',
        icon: (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M3 10h18M7 15h1m4 0h1m-7 4h12a3 3 0 003-3V8a3 3 0 00-3-3H6a3 3 0 00-3 3v8a3 3 0 003 3z" />
            </svg>
        ),
    },
    {
        id: 'contracts',
        label: 'Contracts',
        icon: (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M20 7l-8-4-8 4m16 0l-8 4m8-4v10l-8 4m0-10L4 7m8 4v10M4 7v10l8 4" />
            </svg>
        ),
    },
    {
        id: 'logs',
        label: 'Logs',
        icon: (
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16M4 10h16M4 14h16M4 18h16" />
            </svg>
        ),
    },
];

export function Sidebar({ activeView, onNavigate }: SidebarProps) {
    return (
        <div className="w-56 h-full bg-zinc-900 border-r border-zinc-800 flex flex-col shrink-0">
            {/* Logo */}
            <div className="p-5 border-b border-zinc-800">
                <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center">
                        <svg className="w-6 h-6 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z" />
                        </svg>
                    </div>
                    <div>
                        <h1 className="text-lg font-bold text-white">Isomer</h1>
                        <p className="text-xs text-zinc-500">Alkanes Dev Environment</p>
                    </div>
                </div>
            </div>

            {/* Navigation */}
            <nav className="flex-1 p-3 space-y-1">
                {navItems.map((item) => (
                    <button
                        key={item.id}
                        onClick={() => onNavigate(item.id)}
                        className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg text-left transition-colors ${activeView === item.id
                            ? 'bg-indigo-600/20 text-indigo-400 border border-indigo-600/30'
                            : 'text-zinc-400 hover:text-white hover:bg-zinc-800'
                            }`}
                    >
                        {item.icon}
                        <span className="font-medium">{item.label}</span>
                    </button>
                ))}
            </nav>

            {/* Settings */}
            <div className="p-3 border-t border-zinc-800">
                <button
                    onClick={() => onNavigate('settings')}
                    className={`w-full flex items-center gap-3 px-4 py-2.5 rounded-lg text-left transition-colors ${activeView === 'settings'
                        ? 'bg-indigo-600/20 text-indigo-400 border border-indigo-600/30'
                        : 'text-zinc-400 hover:text-white hover:bg-zinc-800'
                        }`}
                >
                    <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                    </svg>
                    <span className="font-medium">Settings</span>
                </button>
            </div>
        </div>
    );
}

export default Sidebar;
