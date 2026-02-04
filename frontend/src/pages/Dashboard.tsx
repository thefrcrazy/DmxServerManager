import { useState, useEffect, useMemo } from 'react';
import { Link } from 'react-router-dom';
import { Server as ServerIcon, Activity, HardDrive, Users, Plus, Cpu, MemoryStick, Square } from 'lucide-react';
import { formatBytes } from '../utils/formatters';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import ServerList from '../components/ServerList';
import ServerFilters from '../components/ServerFilters';
import { Server } from '../schemas/api';
import { LoadingScreen, StatPill, EmptyState } from '../components/common';

interface ServerStats {
    total: number;
    running: number;
    stopped: number;
}

interface SystemStats {
    cpu: number;
    ram: number;
    ram_used: number;
    ram_total: number;
    disk: number;
    disk_used: number;
    disk_total: number;
    cpu_cores?: number;
    managed_cpu: number;
    managed_cpu_normalized?: number; // Optional as it comes from API
    managed_ram: number;
    managed_disk: number;
}

interface PlayersStats {
    current: number;
    max: number;
}

export default function Dashboard() {
    const { t } = useLanguage();
    const [stats, setStats] = useState<ServerStats>({ total: 0, running: 0, stopped: 0 });
    const [systemStats, setSystemStats] = useState<SystemStats>({
        cpu: 0, ram: 0, ram_used: 0, ram_total: 0, disk: 0, disk_used: 0, disk_total: 0,
        managed_cpu: 0, managed_ram: 0, managed_disk: 0
    });
    const [playersStats, setPlayersStats] = useState<PlayersStats>({ current: 0, max: 0 });
    const [servers, setServers] = useState<Server[]>([]);
    const [isLoading, setIsLoading] = useState(true);

    // Filter states
    const [search, setSearch] = useState('');
    const [gameType, setGameType] = useState('all');
    const [viewMode, setViewMode] = useState<'grid' | 'list'>('list');

    useEffect(() => {
        fetchData();
        // Refresh system stats every 3 seconds
        const statsInterval = setInterval(fetchSystemStats, 3000);
        // Refresh servers every 15 seconds
        const serversInterval = setInterval(fetchServers, 15000);
        return () => {
            clearInterval(statsInterval);
            clearInterval(serversInterval);
        };
    }, []);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t('sidebar.dashboard'), t('dashboard.welcome'));
    }, [setPageTitle, t]);

    const fetchData = async () => {
        try {
            await Promise.all([fetchServers(), fetchSystemStats()]);
        } finally {
            setIsLoading(false);
        }
    };

    const fetchServers = async () => {
        try {
            const response = await fetch('/api/v1/servers', {
                headers: {
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
            });

            if (response.ok) {
                const data: Server[] = await response.json();
                setServers(data);
                setStats({
                    total: data.length,
                    running: data.filter((s) => s.status === 'running').length,
                    stopped: data.filter((s) => s.status === 'stopped').length,
                });
            }
        } catch (error) {
            console.error('Erreur lors du chargement des serveurs:', error);
        }
    };

    const fetchSystemStats = async () => {
        try {
            const response = await fetch('/api/v1/system/stats', {
                headers: {
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
            });

            if (response.ok) {
                const data = await response.json();
                setSystemStats({
                    cpu: data.cpu || 0,
                    ram: data.ram || 0,
                    ram_used: data.ram_used || 0,
                    ram_total: data.ram_total || 0,
                    disk: data.disk || 0,
                    disk_used: data.disk_used || 0,
                    disk_total: data.disk_total || 0,
                    cpu_cores: data.cpu_cores,
                    managed_cpu: data.managed_cpu || 0,
                    managed_cpu_normalized: data.managed_cpu_normalized || 0,
                    managed_ram: data.managed_ram || 0,
                    managed_disk: data.managed_disk || 0,
                });
                setPlayersStats({
                    current: data.players_current || 0,
                    max: data.players_max || 0,
                });
            }
        } catch (error) {
            console.error('Erreur lors du chargement des stats système:', error);
        }
    };

    const handleServerAction = async (id: string, action: 'start' | 'stop' | 'restart' | 'kill') => {
        try {
            await fetch(`/api/v1/servers/${id}/${action}`, {
                method: 'POST',
                headers: {
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
            });
            fetchServers();
            setTimeout(fetchServers, 2000);
        } catch (error) {
            console.error(`Erreur lors de ${action}:`, error);
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

    const getStatColor = (value: number): string => {
        if (value >= 90) return 'danger';
        if (value >= 70) return 'warning';
        return 'success';
    };

    if (isLoading) {
        return <LoadingScreen />;
    }

    // Filter mainly active servers or just show all but using the new component?
    // User asked for "identical display". So I will just show the ServerList.
    // Dashboard usually shows all servers or maybe favorites? I will show all for now.

    return (
        <div className="dashboard-page">
            <div className="dashboard-header-stats">
                <StatPill
                    icon={<ServerIcon size={16} />}
                    label={t('dashboard.total_servers')}
                    value={stats.total}
                    variant="default"
                />
                <StatPill
                    icon={<Activity size={16} />}
                    label={t('servers.status')}
                    value={stats.running}
                    variant="success"
                    sublabel="running"
                />
                <StatPill
                    icon={<Square size={16} />}
                    label={t('servers.stop')}
                    value={stats.stopped}
                    variant="muted"
                    sublabel="stopped"
                />
                <StatPill
                    icon={<Users size={16} />}
                    label={t('servers.players')}
                    value={playersStats.current}
                    variant="purple"
                    suffix={playersStats.max > 0 ? `/${playersStats.max}` : undefined}
                />
            </div>

            <div className="dashboard-grid">
                <div className="card stat-card stat-card--resource">
                    <div className="stat-card__header">
                        <div className="stat-card__title-group">
                            <Cpu size={18} className={`text-${getStatColor(systemStats.cpu)}`} />
                            <span className="stat-card__label">{t('dashboard.cpu_usage')}</span>
                        </div>
                        <span className={`stat-card__value stat-card__value--large text-${getStatColor(systemStats.cpu)}`}>
                            {systemStats.cpu.toFixed(1)}%
                        </span>
                    </div>

                    <div className="stat-card__content">
                        <div className="resource-usage">
                            <div className="resource-usage__row">
                                <span className="resource-usage__label">Global System</span>
                                <span className="resource-usage__value">{systemStats.cpu.toFixed(1)}%</span>
                            </div>
                            <div className="progress-container">
                                <div className={`progress-bar progress-bar--${getStatColor(systemStats.cpu)}`} style={{ width: `${systemStats.cpu}%` }}></div>
                            </div>

                            <div className="resource-usage__row resource-usage__row--managed">
                                <span className="resource-usage__label">Managed Servers</span>
                                <span className="resource-usage__value">{systemStats.managed_cpu.toFixed(1)}%</span>
                            </div>
                            <div className="progress-container progress-container--dimmed">
                                <div className="progress-bar progress-bar--info" style={{ width: `${Math.min(100, systemStats.managed_cpu_normalized || 0)}%` }}></div>
                            </div>
                        </div>
                    </div>
                    <div className="stat-card__footer">
                        <span className="text-muted">{systemStats.cpu_cores ? `${systemStats.cpu_cores} Cores` : '---'}</span>
                    </div>
                </div>

                <div className="card stat-card stat-card--resource">
                    <div className="stat-card__header">
                        <div className="stat-card__title-group">
                            <MemoryStick size={18} className={`text-${getStatColor(systemStats.ram)}`} />
                            <span className="stat-card__label">{t('dashboard.ram_usage')}</span>
                        </div>
                        <span className={`stat-card__value stat-card__value--large text-${getStatColor(systemStats.ram)}`}>
                            {systemStats.ram.toFixed(1)}%
                        </span>
                    </div>

                    <div className="stat-card__content">
                        <div className="resource-usage">
                            <div className="resource-usage__row">
                                <span className="resource-usage__label">Global System</span>
                                <span className="resource-usage__value">{formatBytes(systemStats.ram_used)} / {formatBytes(systemStats.ram_total)}</span>
                            </div>
                            <div className="progress-container">
                                <div className={`progress-bar progress-bar--${getStatColor(systemStats.ram)}`} style={{ width: `${systemStats.ram}%` }}></div>
                            </div>

                            <div className="resource-usage__row resource-usage__row--managed">
                                <span className="resource-usage__label">Managed Servers</span>
                                <span className="resource-usage__value">{formatBytes(systemStats.managed_ram)}</span>
                            </div>
                            <div className="progress-container progress-container--dimmed">
                                <div className="progress-bar progress-bar--info" style={{ width: `${(systemStats.managed_ram / systemStats.ram_total) * 100}%` }}></div>
                            </div>
                        </div>
                    </div>
                </div>

                <div className="card stat-card stat-card--resource">
                    <div className="stat-card__header">
                        <div className="stat-card__title-group">
                            <HardDrive size={18} className={`text-${getStatColor(systemStats.disk)}`} />
                            <span className="stat-card__label">{t('dashboard.disk_usage')}</span>
                        </div>
                        <span className={`stat-card__value stat-card__value--large text-${getStatColor(systemStats.disk)}`}>
                            {systemStats.disk.toFixed(1)}%
                        </span>
                    </div>

                    <div className="stat-card__content">
                        <div className="resource-usage">
                            <div className="resource-usage__row">
                                <span className="resource-usage__label">Global System</span>
                                <span className="resource-usage__value">{formatBytes(systemStats.disk_used)} / {formatBytes(systemStats.disk_total)}</span>
                            </div>
                            <div className="progress-container">
                                <div className={`progress-bar progress-bar--${getStatColor(systemStats.disk)}`} style={{ width: `${systemStats.disk}%` }}></div>
                            </div>

                            <div className="resource-usage__row resource-usage__row--managed">
                                <span className="resource-usage__label">Managed Servers</span>
                                <span className="resource-usage__value">{formatBytes(systemStats.managed_disk)}</span>
                            </div>
                            <div className="progress-container progress-container--dimmed">
                                <div className="progress-bar progress-bar--info" style={{ width: `${(systemStats.managed_disk / systemStats.disk_total) * 100}%` }}></div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div className="dashboard-content">
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
                    <EmptyState
                        icon={<ServerIcon size={32} />}
                        title={t('servers.no_servers')}
                        description={search || gameType !== 'all' ? 'No servers match your filters.' : t('dashboard.welcome')}
                        action={
                            (search === '' && gameType === 'all') && (
                                <Link to="/servers/create" className="btn btn--primary">
                                    <Plus size={18} />
                                    {t('servers.create_new')}
                                </Link>
                            )
                        }
                        className="card mt-4"
                    />
                ) : (
                    <ServerList
                        servers={filteredServers}
                        viewMode={viewMode}
                        onAction={handleServerAction}
                    />
                )}
            </div>
        </div>
    );
}
