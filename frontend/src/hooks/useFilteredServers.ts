import { useState, useCallback, useMemo } from 'react';
import { useServers } from './useServers';

interface UseFilteredServersOptions {
    initialSearch?: string;
    initialGameType?: string;
    initialViewMode?: 'grid' | 'list';
}

export function useFilteredServers(options: UseFilteredServersOptions = {}) {
    const {
        servers,
        loading,
        error,
        refresh,
        startServer,
        stopServer,
        restartServer,
    } = useServers();

    const [search, setSearch] = useState(options.initialSearch || '');
    const [gameType, setGameType] = useState(options.initialGameType || 'all');
    const [viewMode, setViewMode] = useState<'grid' | 'list'>(options.initialViewMode || 'grid');

    const filteredServers = useMemo(() => {
        return servers.filter(server => {
            const matchesSearch = server.name.toLowerCase().includes(search.toLowerCase());
            const matchesGameType = gameType === 'all' || server.game_type === gameType;
            return matchesSearch && matchesGameType;
        });
    }, [servers, search, gameType]);

    const gameTypes = useMemo(() => {
        const types = new Set(servers.map(s => s.game_type));
        return Array.from(types);
    }, [servers]);

    const stats = useMemo(() => ({
        total: servers.length,
        online: servers.filter(s => s.status === 'running').length,
        offline: servers.filter(s => s.status === 'stopped' || s.status === 'offline').length,
    }), [servers]);

    const handleServerAction = useCallback(async (action: 'start' | 'stop' | 'restart', serverId: string) => {
        switch (action) {
            case 'start':
                await startServer(serverId);
                break;
            case 'stop':
                await stopServer(serverId);
                break;
            case 'restart':
                await restartServer(serverId);
                break;
        }
    }, [startServer, stopServer, restartServer]);

    return {
        // Data
        servers: filteredServers,
        allServers: servers,
        loading,
        error,
        stats,
        gameTypes,

        // Filters
        search,
        setSearch,
        gameType,
        setGameType,
        viewMode,
        setViewMode,

        // Actions
        refresh,
        handleServerAction,
    };
}
