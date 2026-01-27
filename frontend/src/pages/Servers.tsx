import { useState, useEffect } from 'react';
import { Link } from 'react-router-dom';
import {
    Plus, Play, Square, RotateCw, Skull, Server as ServerIcon, AlertTriangle
} from 'lucide-react';
import { formatBytes } from '../utils/formatters';
import Table from '../components/Table';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';

interface Server {
    id: string;
    name: string;
    game_type: string;
    status: string;
    executable_path: string;
    working_dir: string;
    auto_start: boolean;
    dir_exists: boolean;
    players?: string[];
    max_players?: number;
    cpu_usage: number;
    memory_usage_bytes: number;
    disk_usage_bytes: number;
}

export default function Servers() {
    const { t } = useLanguage();
    const [servers, setServers] = useState<Server[]>([]);
    const [isLoading, setIsLoading] = useState(true);

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

    if (isLoading) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
            </div>
        );
    }

    return (
        <div className="servers-page">
            <div className="page-header">
                <div>
                    <h1 className="page-header__title">{t('servers.title')}</h1>
                    <p className="page-header__subtitle">Gérez vos instances de serveurs de jeu</p>
                </div>
                <Link to="/servers/create" className="btn btn--primary">
                    <Plus size={18} />
                    {t('servers.create_new')}
                </Link>
            </div>

            {servers.length === 0 ? (
                <div className="empty-state">
                    <div className="empty-state__icon">
                        <ServerIcon size={48} />
                    </div>
                    <h3 className="empty-state__title">Aucun serveur</h3>
                    <p className="empty-state__description">Commencez par créer votre premier serveur Hytale.</p>
                    <Link to="/servers/create" className="btn btn--primary">
                        <Plus size={18} />
                        Créer un serveur
                    </Link>
                </div>

            ) : (
                <Table>
                    <thead>
                        <tr>
                            <th className="col-server">Server</th>
                            <th className="col-actions">Actions</th>
                            <th className="col-cpu">CPU Usage</th>
                            <th className="col-mem">Memory Usage</th>
                            <th className="col-disk">Server Dir Size</th>
                            <th className="col-players">Players</th>
                            <th className="col-status">Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        {servers.map(server => {
                            const isRunning = server.status === 'running';
                            const isMissing = server.status === 'missing';

                            return (
                                <tr key={server.id} className={`server-row ${isRunning ? 'server-row--running' : ''} ${isMissing ? 'server-row--missing' : ''}`}>
                                    <td>
                                        <div className="server-name">
                                            <div className={`server-icon ${isRunning ? 'server-icon--running' : ''} ${isMissing ? 'server-icon--missing' : ''}`}>
                                                {isMissing ? <AlertTriangle size={18} /> : <ServerIcon size={18} />}
                                            </div>
                                            <Link to={`/servers/${server.id}`} className="server-link">
                                                {server.name}
                                            </Link>
                                        </div>
                                    </td>
                                    <td>
                                        <div className="server-actions">
                                            {isMissing ? (
                                                <span className="server-actions__corrupt">
                                                    <AlertTriangle size={14} />
                                                    Corrompu
                                                </span>
                                            ) : isRunning ? (
                                                <>
                                                    <button
                                                        className="btn btn--icon btn--ghost text-info"
                                                        onClick={() => handleAction(server.id, 'restart')}
                                                        title="Redémarrer"
                                                    >
                                                        <RotateCw size={16} />
                                                    </button>
                                                    <button
                                                        className="btn btn--icon btn--ghost text-danger"
                                                        onClick={() => handleAction(server.id, 'stop')}
                                                        title="Arrêter"
                                                    >
                                                        <Square size={16} />
                                                    </button>
                                                    <button
                                                        onClick={() => handleAction(server.id, 'kill')}
                                                        title="Forcer l'arrêt"
                                                        className="btn btn--icon btn--ghost text-danger btn-kill"
                                                    >
                                                        <Skull size={16} />
                                                    </button>
                                                </>
                                            ) : (
                                                <button
                                                    className="btn btn--icon btn--ghost text-success"
                                                    onClick={() => handleAction(server.id, 'start')}
                                                    title="Démarrer"
                                                >
                                                    <Play size={18} />
                                                </button>
                                            )}
                                        </div>
                                    </td>
                                    <td>
                                        <div className="usage-bar">
                                            <div className="usage-bar__track">
                                                <div
                                                    className="usage-bar__fill usage-bar__fill--cpu"
                                                    style={{ width: `${Math.min(100, server.cpu_usage)}%` }}
                                                />
                                            </div>
                                            <span className="usage-bar__text">{server.cpu_usage.toFixed(1)}%</span>
                                        </div>
                                    </td>
                                    <td>
                                        <div className="usage-bar">
                                            <div className="usage-bar__track">
                                                <div
                                                    className="usage-bar__fill usage-bar__fill--mem"
                                                    style={{ width: `${Math.min(100, (server.memory_usage_bytes / 1024 / 1024 / 1024 / 16) * 100)}%` }} // Assuming 16GB scale for visual
                                                />
                                            </div>
                                            <span className="usage-bar__text">
                                                {formatBytes(server.memory_usage_bytes)}
                                            </span>
                                        </div>
                                    </td>
                                    <td className="text-cell">
                                        {server.disk_usage_bytes > 0 ? formatBytes(server.disk_usage_bytes) : '--'}
                                    </td>
                                    <td className="text-cell">
                                        {server.players ? `${server.players.length}` : '0'} / {server.max_players || '?'} Max
                                    </td>
                                    <td className="text-right">
                                        <span className={`badge badge--${isMissing ? 'warning' : server.status === 'running' ? 'success' : 'danger'}`}>
                                            {isMissing ? 'Missing' : server.status === 'running' ? 'Online' : 'Offline'}
                                        </span>
                                    </td>
                                </tr>
                            );
                        })}
                    </tbody>
                </Table>
            )}
        </div>
    );
}
