import { create } from "zustand";
import type { SystemStatus, Account, BinaryInfo, IsomerConfig } from "./types";

interface IsomerState {
  // System status
  status: SystemStatus | null;
  isLoading: boolean;
  error: string | null;

  // Accounts
  accounts: Account[];

  // Binaries
  binaries: BinaryInfo[];
  downloadProgress: Record<string, number>;

  // Config
  config: IsomerConfig | null;

  // Actions
  setStatus: (status: SystemStatus) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setAccounts: (accounts: Account[]) => void;
  setBinaries: (binaries: BinaryInfo[]) => void;
  setDownloadProgress: (service: string, progress: number) => void;
  setConfig: (config: IsomerConfig) => void;
}

export const useStore = create<IsomerState>((set) => ({
  // Initial state
  status: null,
  isLoading: false,
  error: null,
  accounts: [],
  binaries: [],
  downloadProgress: {},
  config: null,

  // Actions
  setStatus: (status) => set({ status }),
  setLoading: (isLoading) => set({ isLoading }),
  setError: (error) => set({ error }),
  setAccounts: (accounts) => set({ accounts }),
  setBinaries: (binaries) => set({ binaries }),
  setDownloadProgress: (service, progress) =>
    set((state) => ({
      downloadProgress: { ...state.downloadProgress, [service]: progress },
    })),
  setConfig: (config) => set({ config }),
}));

export default useStore;
