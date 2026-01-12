import { useEffect, useState, useCallback } from 'react';
import { useSystemStatus, useBinaries } from './hooks/useStatus';
import { invoke } from '@tauri-apps/api/core';

// New Layout Components
import { AppLayout } from './components/Layout/AppLayout';
import { Header } from './components/Layout/Header';
import { ControlPlane } from './components/Layout/ControlPlane';
import { ServiceMatrix } from './components/Layout/ServiceMatrix';
import { Diagnostics } from './components/Layout/Diagnostics';
import { SetupScreen } from './components/SetupScreen';
import { ExtensionPanel } from './components/Extension/ExtensionPanel';

function App() {
  const {
    services,
    serviceList,
    blockHeight,
    mempoolSize,
    isMining,
    refreshStatus
  } = useSystemStatus(2000);

  const { binaries, checkBinaries } = useBinaries();

  // Optimistic/Local loading states
  const [transitionTarget, setTransitionTarget] = useState<'running' | 'stopped' | null>(null);
  const [isMiningLocally, setIsMiningLocally] = useState(false);

  // Extension State
  const [extensionPanelOpen, setExtensionPanelOpen] = useState(false);
  const [isExtensionInstalled, setIsExtensionInstalled] = useState(false);
  const [extensionPath, setExtensionPath] = useState<string | null>(null);
  const [isDownloadingExtension, setIsDownloadingExtension] = useState(false);

  // Check binaries on mount
  useEffect(() => {
    checkBinaries();
    checkExtension();
  }, []);

  const checkExtension = async () => {
    try {
      const installed = await invoke<boolean>('check_extension_status');
      setIsExtensionInstalled(installed);

      // If installed, we can safely get the path without triggering a download
      if (installed) {
        // We use the same command but rely on it returning quickly if installed.
        // NOTE: get_extension_path logic is "if installed return path, else download".
        // Since we know it's installed, this is safe/free.
        const path = await invoke<string>('get_extension_path');
        setExtensionPath(path);
      }
    } catch (e) {
      console.error('Failed to check extension status:', e);
    }
  };

  const handleDownloadExtension = async () => {
    if (isDownloadingExtension) return;
    setIsDownloadingExtension(true);
    try {
      const path = await invoke<string>('get_extension_path');
      setExtensionPath(path);
      setIsExtensionInstalled(true);
    } catch (e) {
      console.error('Failed to download extension:', e);
      alert(`Failed to download extension: ${e}`);
    } finally {
      setIsDownloadingExtension(false);
    }
  };

  // Determine global system state
  const isSystemRunning = services.bitcoind === 'running';

  // Reset transition state when backend matches target
  useEffect(() => {
    if (!transitionTarget) return;

    if (transitionTarget === 'running' && services.bitcoind === 'running') {
      console.log('[App] Target reached: running');
      setTransitionTarget(null);
    } else if (transitionTarget === 'stopped' && services.bitcoind === 'stopped') {
      console.log('[App] Target reached: stopped');
      setTransitionTarget(null);
    } else if (services.bitcoind === 'error') {
      console.log('[App] Service error during transition');
      setTransitionTarget(null);
    }
  }, [services.bitcoind, transitionTarget]);

  useEffect(() => {
    if (isMining) setIsMiningLocally(false);
  }, [isMining]);

  // Aggregate status for the Control Plane
  const globalStatus = transitionTarget
    ? (transitionTarget === 'running' ? 'starting' : 'stopping')
    : (services.bitcoind as any);

  const hasMissingBinaries = binaries.some(b => b.status === 'notinstalled');

  if (hasMissingBinaries) {
    return <SetupScreen />;
  }

  // Handlers
  const handleStartStop = useCallback(async () => {
    console.log('[App] handleStartStop called. Current isSystemRunning:', isSystemRunning);
    const target = isSystemRunning ? 'stopped' : 'running';
    console.log('[App] Setting transition target to:', target);
    setTransitionTarget(target);

    try {
      if (isSystemRunning) {
        console.log('[App] Invoking stop_services');
        await invoke('stop_services');
      } else {
        console.log('[App] Invoking start_services');
        await invoke('start_services');
      }
      refreshStatus();
    } catch (e) {
      console.error('[App] Action failed:', e);
      setTransitionTarget(null);
    }
  }, [isSystemRunning, refreshStatus]);

  const handleMine = useCallback(async () => {
    if (!isSystemRunning || isMiningLocally) return;
    setIsMiningLocally(true);
    try {
      await invoke('mine_blocks', { count: 1 });
      refreshStatus();
      window.dispatchEvent(new CustomEvent('isomer:mine'));
      setTimeout(() => setIsMiningLocally(false), 800);
    } catch (e) {
      console.error('Failed to mine block:', e);
      setIsMiningLocally(false);
    }
  }, [isSystemRunning, isMiningLocally, refreshStatus]);

  // Global Shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        handleMine();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleMine]);

  return (
    <>
      <AppLayout
        header={
          <Header
            blockHeight={blockHeight}
            mempoolSize={mempoolSize}
            isSystemHealth={isSystemRunning}
          />
        }
        hero={
          <ControlPlane
            serviceStatus={globalStatus}
            isMining={isMining || isMiningLocally}
            onMine={handleMine}
            onStartStop={handleStartStop}
          />
        }
        main={
          <ServiceMatrix
            status={globalStatus}
            services={serviceList}
            extensionStatus={{
              installed: isExtensionInstalled,
              onOpen: () => setExtensionPanelOpen(true)
            }}
          />
        }
        bottom={
          <Diagnostics services={serviceList} />
        }
      />

      <ExtensionPanel
        isOpen={extensionPanelOpen}
        onClose={() => setExtensionPanelOpen(false)}
        extensionPath={extensionPath}
        onDownload={handleDownloadExtension}
        isDownloading={isDownloadingExtension}
      />
    </>
  );
}

export default App;
