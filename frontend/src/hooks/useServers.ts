import { useState, useEffect, useCallback, useMemo } from "react";
import apiService from "../services/api";
import { Server } from "../schemas/api";

interface UseServersReturn {
    servers: Server[];
    loading: boolean;
    error: string | null;
    refresh: () => Promise<void>;
    startServer: (id: string) => Promise<boolean>;
    stopServer: (id: string) => Promise<boolean>;
    restartServer: (id: string) => Promise<boolean>;
    reinstallServer: (id: string) => Promise<boolean>;
    deleteServer: (id: string) => Promise<boolean>;
    killServer: (id: string) => Promise<boolean>;
    createServer: (data: Omit<Server, "id" | "status" | "created_at" | "updated_at">) => Promise<string | null>;
    // Computed values
    onlineCount: number;
    offlineCount: number;
}

export function useServers(): UseServersReturn {
    const [servers, setServers] = useState<Server[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const response = await apiService.servers.getServers();
            setServers(response.data || []);
        } catch (err) {
            setError(err instanceof Error ? err.message : "Erreur de chargement");
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        refresh();
    }, [refresh]);

    const startServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.startServer(id);
            await refresh();
            return true;
        } catch {
            return false;
        }
    }, [refresh]);

    const stopServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.stopServer(id);
            await refresh();
            return true;
        } catch {
            return false;
        }
    }, [refresh]);

    const restartServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.restartServer(id);
            await refresh();
            return true;
        } catch {
            return false;
        }
    }, [refresh]);

    const deleteServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.deleteServer(id);
            setServers(prev => prev.filter(s => s.id !== id));
            return true;
        } catch {
            return false;
        }
    }, []);

    const killServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.killServer(id);
            await refresh();
            return true;
        } catch {
            return false;
        }
    }, [refresh]);

    const reinstallServer = useCallback(async (id: string): Promise<boolean> => {
        try {
            await apiService.servers.reinstallServer(id);
            // Status might need update but usually it stays 'running' or 'stopped' until process manager picks it up
            return true;
        } catch {
            return false;
        }
    }, []);

    const createServer = useCallback(async (data: any): Promise<string | null> => {
        try {
            const response = await apiService.servers.createServer(data);
            await refresh();
            return response.data?.id || null;
        } catch {
            return null;
        }
    }, [refresh]);

    const onlineCount = useMemo(() =>
        servers.filter(s => s.status === "running").length,
        [servers]
    );

    const offlineCount = useMemo(() =>
        servers.filter(s => s.status === "stopped").length,
        [servers]
    );

    return {
        servers,
        loading,
        error,
        refresh,
        startServer,
        stopServer,
        restartServer,
        reinstallServer,
        deleteServer,
        killServer,
        createServer,
        onlineCount,
        offlineCount,
    };
}
