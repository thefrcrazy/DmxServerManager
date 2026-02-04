import { useEffect, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { Plus, Server as ServerIcon } from 'lucide-react';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import ServerList from '../components/ServerList';
import ServerFilters from '../components/ServerFilters';
import { useFilteredServers } from '../hooks';
import { LoadingScreen, EmptyState } from '../components/common';

export default function Servers() {
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();

    const {
        servers,
        loading,
        gameTypes,
        search,
        setSearch,
        gameType,
        setGameType,
        viewMode,
        setViewMode,
        handleServerAction,
    } = useFilteredServers({ initialViewMode: 'list' });

    useEffect(() => {
        setPageTitle(t('servers.title'), t('dashboard.welcome'), { to: '/' });
    }, [setPageTitle, t]);

    // Adapter function to match ServerList's expected signature
    const onAction = useCallback((id: string, action: 'start' | 'stop' | 'restart' | 'kill') => {
        if (action === 'kill') return; // kill not supported in useFilteredServers
        handleServerAction(action, id);
    }, [handleServerAction]);

    if (loading) {
        return <LoadingScreen />;
    }

    return (
        <div className="servers-page">
            <ServerFilters
                search={search}
                onSearchChange={setSearch}
                gameType={gameType}
                onGameTypeChange={setGameType}
                viewMode={viewMode}
                onViewModeChange={setViewMode}
                gameTypes={gameTypes}
                action={
                    <Link to="/servers/create" className="btn btn--primary">
                        <Plus size={18} />
                        {t('servers.create_new')}
                    </Link>
                }
            />

            {servers.length === 0 ? (
                <EmptyState
                    icon={<ServerIcon size={48} />}
                    title={t('servers.no_servers')}
                    description={search || gameType !== 'all' ? 'No servers match your filters.' : t('servers.empty_desc')}
                    action={
                        (search === '' && gameType === 'all') && (
                            <Link to="/servers/create" className="btn btn--primary">
                                <Plus size={18} />
                                {t('servers.create_new')}
                            </Link>
                        )
                    }
                />
            ) : (
                <ServerList
                    servers={servers}
                    viewMode={viewMode}
                    onAction={onAction}
                />
            )}
        </div>
    );
}
