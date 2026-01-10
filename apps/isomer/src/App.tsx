import { useState, useEffect } from 'react';
import { Sidebar } from './components/Sidebar';
import { Dashboard } from './components/Dashboard';
import { LogsPanel } from './components/LogsPanel';
import { SettingsPanel } from './components/SettingsPanel';
import { ExplorerPanel } from './components/ExplorerPanel';
import { SetupScreen } from './components/SetupScreen';
import { useSystemStatus, useBinaries } from './hooks/useStatus';

function App() {
  const [activeView, setActiveView] = useState('dashboard');
  const { binaries, checkBinaries } = useBinaries();

  // Start polling for system status
  useSystemStatus(2000);

  // Check binaries on mount
  useEffect(() => {
    checkBinaries();
  }, []);

  const hasMissingBinaries = binaries.some(b => b.status === 'notinstalled');

  if (hasMissingBinaries) {
    return <SetupScreen />;
  }

  return (
    <div className="flex h-screen bg-zinc-950">
      <Sidebar activeView={activeView} onNavigate={setActiveView} />

      <main className="flex-1 overflow-auto">
        {activeView === 'dashboard' && <Dashboard />}
        {activeView === 'accounts' && <PlaceholderView title="Accounts" />}
        {activeView === 'explorer' && <ExplorerPanel />}
        {activeView === 'contracts' && <PlaceholderView title="Contracts" />}
        {activeView === 'logs' && <LogsPanel />}
        {activeView === 'settings' && <SettingsPanel />}
      </main>
    </div>
  );
}

// Placeholder for views we haven't implemented yet
function PlaceholderView({ title }: { title: string }) {
  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-center">
        <div className="w-16 h-16 rounded-2xl bg-zinc-800 flex items-center justify-center mx-auto mb-4">
          <svg className="w-8 h-8 text-zinc-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
          </svg>
        </div>
        <h2 className="text-xl font-semibold text-white mb-2">{title}</h2>
        <p className="text-zinc-500">Coming soon...</p>
      </div>
    </div>
  );
}

export default App;
