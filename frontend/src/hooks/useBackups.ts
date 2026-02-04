import { useState, useEffect, useCallback } from "react";
import apiService from "../services/api";

export interface Backup {
    id: string;
    server_id: string;
    filename: string;
    size_bytes: number;
    created_at: string;
}

interface UseBackupsReturn {
    backups: Backup[];
    loading: boolean;
    error: string | null;
    refresh: (serverId?: string) => Promise<void>;
    createBackup: (serverId: string) => Promise<Backup | null>;
    deleteBackup: (id: string) => Promise<boolean>;
    restoreBackup: (id: string) => Promise<boolean>;
    // Helpers
    formatSize: (bytes: number) => string;
    getBackupsForServer: (serverId: string) => Backup[];
}

export function useBackups(initialServerId?: string): UseBackupsReturn {
    const [backups, setBackups] = useState<Backup[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async (serverId?: string) => {
        setLoading(true);
        setError(null);
        try {
            const response = await apiService.backups.getBackups(serverId);
            setBackups(response.data || []);
        } catch (err) {
            setError(err instanceof Error ? err.message : "Erreur de chargement");
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        refresh(initialServerId);
    }, [refresh, initialServerId]);

    const createBackup = useCallback(async (serverId: string): Promise<Backup | null> => {
        try {
            const response = await apiService.backups.createBackup(serverId);
            if (response.data) {
                setBackups(prev => [response.data, ...prev]);
                return response.data;
            }
            return null;
        } catch {
            return null;
        }
    }, []);

    const deleteBackup = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.backups.deleteBackup(id);
            setBackups(prev => prev.filter(b => b.id !== id));
            return true;
        } catch {
            return false;
        }
    }, []);

    const restoreBackup = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.backups.restoreBackup(id);
            return true;
        } catch {
            return false;
        }
    }, []);

    const formatSize = useCallback((bytes: number): string => {
        if (bytes === 0) return "0 B";
        const k = 1024;
        const sizes = ["B", "KB", "MB", "GB"];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
    }, []);

    const getBackupsForServer = useCallback((serverId: string): Backup[] => {
        return backups.filter(b => b.server_id === serverId);
    }, [backups]);

    return {
        backups,
        loading,
        error,
        refresh,
        createBackup,
        deleteBackup,
        restoreBackup,
        formatSize,
        getBackupsForServer,
    };
}
