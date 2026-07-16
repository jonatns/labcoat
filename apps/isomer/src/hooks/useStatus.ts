import { useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import { useStore } from "../lib/store";

/**
 * Hook to poll system status and listen for events
 */
export function useSystemStatus(pollInterval = 2000) {
  const { status, setStatus, setError, setDownloadProgress, setServiceHealth } =
    useStore();

  const services = status?.services || [];
  const blockHeight = status?.block_height || 0;
  const mempoolSize = status?.mempool_size || 0;
  const isMining = false; // TODO: Implement mining status derivation

  const fetchStatus = useCallback(
    async (mounted = true) => {
      try {
        const status = await api.getStatus();
        if (mounted) {
          setStatus(status);
          setError(null);

          // Poll health for running services
          status.services.forEach(async (service) => {
            if (service.status === "running") {
              try {
                const isHealthy = await api.checkServiceHealth(service.id);
                if (mounted) setServiceHealth(service.id, isHealthy);
              } catch (e) {
                console.warn(`Health check failed for ${service.name}`, e);
                if (mounted) setServiceHealth(service.id, false);
              }
            } else {
              if (mounted) setServiceHealth(service.id, false);
            }
          });
        }
      } catch (err) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to get status");
        }
      }
    },
    [setStatus, setError, setServiceHealth]
  );

  useEffect(() => {
    let mounted = true;

    // Initial fetch
    fetchStatus(mounted);

    // Poll for updates
    const interval = setInterval(() => fetchStatus(mounted), pollInterval);

    // Listen for download progress events
    const unlisten = listen<{ service: string; progress: number }>(
      "download-progress",
      (event) => {
        setDownloadProgress(event.payload.service, event.payload.progress);
      }
    );

    return () => {
      mounted = false;
      clearInterval(interval);
      unlisten.then((fn) => fn());
    };
  }, [pollInterval, fetchStatus, setDownloadProgress]);

  return {
    services: {
      bitcoind:
        (services.find((s) => s.id === "bitcoind")?.status as any) || "stopped",
    },
    serviceList: services,
    blockHeight,
    mempoolSize,
    isMining,
    refreshStatus: () => fetchStatus(true),
  };
}

/**
 * Hook to check and download binaries
 */
export function useBinaries() {
  const { binaries, setBinaries, setLoading, setError } = useStore();

  const checkBinaries = async () => {
    setLoading(true);
    try {
      const result = await api.checkBinaries();
      setBinaries(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to check binaries");
    } finally {
      setLoading(false);
    }
  };

  const downloadBinaries = async () => {
    setLoading(true);
    try {
      await api.downloadBinaries();
      await checkBinaries(); // Refresh status after download
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to download binaries"
      );
    } finally {
      setLoading(false);
    }
  };

  return { binaries, checkBinaries, downloadBinaries };
}
