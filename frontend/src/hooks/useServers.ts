import { useCallback, useEffect, useMemo, useState } from "react";
import { GameProfile, Instance } from "@/schemas/api";
import apiService from "@/services/api";
import { CreateServerInput, ServerAction } from "@/services/api/server.client";

export function useServers() {
    const [servers, setServers] = useState<Instance[]>([]);
    const [profiles, setProfiles] = useState<GameProfile[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        setLoading(true);
        const [response, profilesResponse] = await Promise.all([
            apiService.servers.getServers(),
            apiService.profiles.getProfiles(),
        ]);
        if (response.success) {
            setServers(response.data);
            setError(null);
        } else {
            setError(response.error.message);
        }
        if (profilesResponse.success) setProfiles(profilesResponse.data);
        setLoading(false);
    }, []);

    useEffect(() => { void refresh(); }, [refresh]);

    const runAction = useCallback(async (id: string, action: ServerAction): Promise<boolean> => {
        const response = await apiService.servers.runAction(id, action);
        if (!response.success) {
            setError(response.error.message);
            return false;
        }
        await refresh();
        return true;
    }, [refresh]);

    const deleteServer = useCallback(async (id: string): Promise<boolean> => {
        const response = await apiService.servers.deleteServer(id);
        if (!response.success) {
            setError(response.error.message);
            return false;
        }
        setServers((current) => current.filter((server) => server.id !== id));
        return true;
    }, []);

    const createServer = useCallback(async (data: CreateServerInput): Promise<string | null> => {
        const response = await apiService.servers.createServer(data);
        if (!response.success) {
            setError(response.error.message);
            return null;
        }
        setServers((current) => [response.data, ...current]);
        return response.data.id;
    }, []);

    const onlineCount = useMemo(
        () => servers.filter((server) => server.runtime_state === "running").length,
        [servers],
    );
    const offlineCount = useMemo(
        () => servers.filter((server) => server.runtime_state !== "running").length,
        [servers],
    );

    return {
        servers,
        profiles,
        loading,
        error,
        refresh,
        runAction,
        deleteServer,
        createServer,
        onlineCount,
        offlineCount,
    };
}
