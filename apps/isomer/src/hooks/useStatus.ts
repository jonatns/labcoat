import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "../lib/api";
import { useStore } from "../lib/store";

/**
 * Hook to poll system status and listen for events
 */
export function useSystemStatus(pollInterval = 2000) {
  const { setStatus, setError, setDownloadProgress } = useStore();

  useEffect(() => {
    let mounted = true;

    // Initial fetch
    const fetchStatus = async () => {
      try {
        const status = await api.getStatus();
        if (mounted) {
          setStatus(status);
          setError(null);
        }
      } catch (err) {
        if (mounted) {
          setError(err instanceof Error ? err.message : "Failed to get status");
        }
      }
    };

    fetchStatus();

    // Poll for updates
    const interval = setInterval(fetchStatus, pollInterval);

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
  }, [pollInterval, setStatus, setError, setDownloadProgress]);
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
