import { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { Server, Activity, HardDrive, Users, Plus, Play, Square, Cpu, MemoryStick } from 'lucide-react';
import { formatBytes } from '../utils/formatters';

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
}

interface PlayersStats {
    current: number;
    max: number;
}

interface ServerInfo {
    id: string;
    name: string;
    game_type: string;
    status: string;
    players?: string[];
    max_players?: number;
}

import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';

export default function Dashboard() {
    const { t } = useLanguage();
    const [stats, setStats] = useState<ServerStats>({ total: 0, running: 0, stopped: 0 });
    // ... (rest of state initialization)
    const [systemStats, setSystemStats] = useState<SystemStats>({
        cpu: 0, ram: 0, ram_used: 0, ram_total: 0, disk: 0, disk_used: 0, disk_total: 0
    });
    const [playersStats, setPlayersStats] = useState<PlayersStats>({ current: 0, max: 0 });
    const [servers, setServers] = useState<ServerInfo[]>([]);
    const [isLoading, setIsLoading] = useState(true);

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

    // ... (rest of functions: fetchData, fetchServers, fetchSystemStats, handleServerAction)

    const fetchData = async () => {
        try {
            await Promise.all([fetchServers(), fetchSystemStats()]);
        } finally {
            setIsLoading(false);
        }
    };

    const fetchServers = async () => {
        // ... (no changes in fetch logic)
        try {
            const response = await fetch('/api/v1/servers', {
                headers: {
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
            });

            if (response.ok) {
                const data: ServerInfo[] = await response.json();
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
        // ... (no changes here)
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

    // Quick helpers
    const handleServerAction = async (id: string, action: 'start' | 'stop') => {
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

    const getStatColor = (value: number): string => {
        if (value >= 90) return 'danger';
        if (value >= 70) return 'warning';
        return 'success';
    };

    if (isLoading) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
                <p className="text-muted">{t('common.loading')}</p>
            </div>
        );
    }

    return (
        <div>
            {/* Server Stats */}
            <div className="stats-grid stats-grid--4col">
                <div className="card stat-card">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('dashboard.total_servers')}</div>
                        <div className="stat-card__icon stat-card__icon--default">
                            <Server size={20} />
                        </div>
                    </div>
                    <div className="stat-card__value">{stats.total}</div>
                </div>

                <div className="card stat-card">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('servers.status')}</div>
                        <div className="stat-card__icon stat-card__icon--success">
                            <Activity size={20} />
                        </div>
                    </div>
                    <div className="stat-card__value stat-card__value--success">{stats.running}</div>
                </div>

                <div className="card stat-card">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('servers.stop')}</div>
                        <div className="stat-card__icon stat-card__icon--default">
                            <Square size={20} />
                        </div>
                    </div>
                    <div className="stat-card__value stat-card__value--muted">{stats.stopped}</div>
                </div>

                <div className="card stat-card">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('servers.players')}</div>
                        <div className="stat-card__icon stat-card__icon--purple">
                            <Users size={20} />
                        </div>
                    </div>
                    <div className="stat-card__value">
                        {playersStats.current}
                        {playersStats.max > 0 && <span className="stat-card__value--suffix">/{playersStats.max}</span>}
                    </div>
                </div>
            </div>

            {/* System Stats */}
            <div className="section-header">
                <h2 className="section-title">{t('dashboard.system_status')}</h2>
                {stats.running > 0 && (
                    <span className="section-badge">
                        {stats.running} serveur{stats.running > 1 ? 's' : ''} actif{stats.running > 1 ? 's' : ''}
                    </span>
                )}
            </div>
            <div className="stats-grid stats-grid--3col">
                <div className="card stat-card stat-card--progress">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('dashboard.cpu_usage')}</div>
                        <div className={`stat-card__icon stat-card__icon--${getStatColor(systemStats.cpu)}`}>
                            <Cpu size={20} />
                        </div>
                    </div>
                    <div className={`stat-card__value stat-card__value--${getStatColor(systemStats.cpu)}`}>
                        {systemStats.cpu.toFixed(1)}%
                    </div>
                    <div className="stat-card__progress">
                        <div
                            className={`stat-card__progress-bar stat-card__progress-bar--${getStatColor(systemStats.cpu)}`}
                            style={{ width: `${systemStats.cpu}%` }}
                        />
                    </div>
                </div>

                <div className="card stat-card stat-card--progress">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('dashboard.ram_usage')}</div>
                        <div className={`stat-card__icon stat-card__icon--${getStatColor(systemStats.ram)}`}>
                            <MemoryStick size={20} />
                        </div>
                    </div>
                    <div className={`stat-card__value stat-card__value--${getStatColor(systemStats.ram)}`}>
                        {systemStats.ram.toFixed(1)}%
                    </div>
                    <div className="stat-card__meta">
                        {formatBytes(systemStats.ram_used)} / {formatBytes(systemStats.ram_total)}
                    </div>
                    <div className="stat-card__progress">
                        <div
                            className={`stat-card__progress-bar stat-card__progress-bar--${getStatColor(systemStats.ram)}`}
                            style={{ width: `${systemStats.ram}%` }}
                        />
                    </div>
                </div>

                <div className="card stat-card stat-card--progress">
                    <div className="stat-card__header">
                        <div className="stat-card__label">{t('dashboard.disk_usage')}</div>
                        <div className={`stat-card__icon stat-card__icon--${getStatColor(systemStats.disk)}`}>
                            <HardDrive size={20} />
                        </div>
                    </div>
                    <div className={`stat-card__value stat-card__value--${getStatColor(systemStats.disk)}`}>
                        {systemStats.disk.toFixed(1)}%
                    </div>
                    <div className="stat-card__meta">
                        {formatBytes(systemStats.disk_used)} / {formatBytes(systemStats.disk_total)}
                    </div>
                    <div className="stat-card__progress">
                        <div
                            className={`stat-card__progress-bar stat-card__progress-bar--${getStatColor(systemStats.disk)}`}
                            style={{ width: `${systemStats.disk}%` }}
                        />
                    </div>
                </div>
            </div>

            <h2 className="section-title">{t('dashboard.active_servers')}</h2>

            {servers.length === 0 ? (
                <div className="card empty-state">
                    <div className="empty-state__icon">
                        <Server size={32} />
                    </div>
                    <h3 className="empty-state__title">{t('servers.no_servers')}</h3>
                    <p className="empty-state__description">
                        {t('dashboard.welcome')}
                    </p>
                    <Link to="/servers" className="btn btn--primary">
                        <Plus size={18} />
                        {t('servers.create_new')}
                    </Link>
                </div>
            ) : (
                <div className="servers-list">
                    {servers.map((server) => (
                        <div
                            key={server.id}
                            className={`card server-item server-item--${server.status === 'running' ? 'running' : 'stopped'}`}
                        >
                            <div className="server-item__info">
                                <div className="server-item__icon">
                                    {server.game_type === 'hytale'
                                        ? <img src="https://hytale.com/favicon.ico" alt="H" width="24" />
                                        : <Server size={20} />
                                    }
                                </div>
                                <div>
                                    <div className="server-item__name">{server.name}</div>
                                    <div className="server-item__meta">
                                        <span>{server.game_type}</span>
                                        <span>•</span>
                                        <span className={server.status === 'running' ? 'server-item__status--running' : ''}>
                                            {server.status === 'running' ? 'En ligne' : 'Arrêté'}
                                        </span>

                                        <>
                                            <span className="server-item__players">
                                                <Users size={14} />
                                                Joueur(s): {server.players ? server.players.length : 0}
                                            </span>
                                        </>
                                    </div>
                                </div>
                            </div>

                            <div className="server-item__actions">
                                <Link to={`/servers/${server.id}`} className="btn btn--secondary btn--sm">
                                    Console
                                </Link>

                                {server.status === 'stopped' ? (
                                    <button className="btn btn--success btn--sm" onClick={() => handleServerAction(server.id, 'start')}>
                                        <Play size={16} />
                                    </button>
                                ) : (
                                    <button className="btn btn--danger btn--sm" onClick={() => handleServerAction(server.id, 'stop')}>
                                        <Square size={16} />
                                    </button>
                                )}
                            </div>
                        </div>
                    ))}
                </div>
            )
            }
        </div >
    );
}
