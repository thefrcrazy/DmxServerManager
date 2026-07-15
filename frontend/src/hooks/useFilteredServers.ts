import { useCallback, useMemo, useState } from "react";
import { ServerAction } from "@/services/api/server.client";
import { useServers } from "./useServers";

interface UseFilteredServersOptions {
    initialSearch?: string;
    initialProfile?: string;
    initialViewMode?: "grid" | "list";
}

export function useFilteredServers(options: UseFilteredServersOptions = {}) {
    const serverState = useServers();
    const [search, setSearch] = useState(options.initialSearch ?? "");
    const [profileId, setProfileId] = useState(options.initialProfile ?? "all");
    const [viewMode, setViewMode] = useState<"grid" | "list">(options.initialViewMode ?? "grid");

    const servers = useMemo(() => serverState.servers.filter((server) => (
        server.name.toLocaleLowerCase().includes(search.toLocaleLowerCase())
        && (profileId === "all" || server.profile_id === profileId)
    )), [profileId, search, serverState.servers]);

    const profileIds = useMemo(
        () => [...new Set(serverState.servers.map((server) => server.profile_id))].sort(),
        [serverState.servers],
    );

    const stats = useMemo(() => ({
        total: serverState.servers.length,
        online: serverState.onlineCount,
        offline: serverState.offlineCount,
    }), [serverState.offlineCount, serverState.onlineCount, serverState.servers.length]);

    const handleServerAction = useCallback(
        (action: ServerAction, serverId: string) => serverState.runAction(serverId, action),
        [serverState],
    );

    return {
        ...serverState,
        servers,
        stats,
        profileIds,
        search,
        setSearch,
        gameType: profileId,
        setGameType: setProfileId,
        viewMode,
        setViewMode,
        handleServerAction,
    };
}
