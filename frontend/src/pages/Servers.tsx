import { useState, useEffect, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { Plus, Server as ServerIcon } from 'lucide-react';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import ServerList from '../components/ServerList';
import ServerFilters from '../components/ServerFilters';
import { Server } from '../schemas/api';

export default function Servers() {
    const { t } = useLanguage();
    const [servers, setServers] = useState<Server[]>([]);
    const [isLoading, setIsLoading] = useState(true);

    // Filter states
    const [search, setSearch] = useState('');
    const [gameType, setGameType] = useState('all');
    const [viewMode, setViewMode] = useState<'grid' | 'list'>('list');

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t('servers.title'), t('dashboard.welcome'), { to: '/' });
    }, [setPageTitle, t]);

    useEffect(() => {
        fetchServers();
        const interval = setInterval(fetchServers, 5000);
        return () => clearInterval(interval);
    }, []);

    const fetchServers = async () => {
        try {
            const response = await fetch('/api/v1/servers', {
                headers: {
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
            });
            if (response.ok) {
                setServers(await response.json());
            }
        } catch (error) {
            console.error('Erreur:', error);
        } finally {
            setIsLoading(false);
        }
    };

    const handleAction = async (id: string, action: 'start' | 'stop' | 'restart' | 'kill') => {
        try {
            await fetch(`/api/v1/servers/${id}/${action}`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            fetchServers();
        } catch (e) {
            console.error(e);
        }
    };

    const uniqueGameTypes = useMemo(() => {
        const types = new Set(servers.map(s => s.game_type));
        return Array.from(types);
    }, [servers]);

    const filteredServers = useMemo(() => {
        return servers.filter(server => {
            const matchesSearch = server.name.toLowerCase().includes(search.toLowerCase());
            const matchesType = gameType === 'all' || server.game_type === gameType;
            return matchesSearch && matchesType;
        });
    }, [servers, search, gameType]);

    if (isLoading) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
            </div>
        );
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
                gameTypes={uniqueGameTypes}
                action={
                    <Link to="/servers/create" className="btn btn--primary">
                        <Plus size={18} />
                        {t('servers.create_new')}
                    </Link>
                }
            />

            {filteredServers.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-state__icon">
                        <ServerIcon size={48} />
                    </div>
                    <h3 className="empty-state__title">{t('servers.no_servers')}</h3>
                    <p className="empty-state__description">
                        {search || gameType !== 'all' ? 'No servers match your filters.' : t('servers.empty_desc')}
                    </p>
                    {(search === '' && gameType === 'all') && (
                        <Link to="/servers/create" className="btn btn--primary">
                            <Plus size={18} />
                            {t('servers.create_new')}
                        </Link>
                    )}
                </div>
            ) : (
                <ServerList
                    servers={filteredServers}
                    viewMode={viewMode}
                    onAction={handleAction}
                />
            )}
        </div>
    );
}
