import { useState, useEffect, useRef, useCallback } from 'react';
import { useParams } from 'react-router-dom';
import {
    Terminal, Clock, Cpu, HardDrive, Users, Globe,
    FileText, Settings, History, FolderOpen, BarChart3,
    Webhook, Calendar, Plus, Download, Trash2,
    File, Folder, ChevronRight, Save, AlertCircle, Check,
    ChevronDown, Server as ServerIcon
} from 'lucide-react';
import Select from '../components/Select';
import Checkbox from '../components/Checkbox';
import RangeSlider from '../components/RangeSlider';

import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';

interface Backup {
    id: string;
    server_id: string;
    filename: string;
    size_bytes: number;
    created_at: string;
}

interface FileEntry {
    name: string;
    path: string;
    is_dir: boolean;
    size?: number;
}

interface Server {
    id: string;
    name: string;
    game_type: string;
    status: string;
    working_dir: string;
    executable_path: string;
    // CLI Arguments
    min_memory?: string;
    max_memory?: string;
    java_path?: string;
    extra_args?: string;
    assets_path?: string;
    accept_early_plugins?: boolean;
    auto_start?: boolean;
    disable_sentry?: boolean;
    bind_address?: string;
    port?: number;
    auth_mode?: 'authenticated' | 'offline';
    allow_op?: boolean;
    backup_enabled?: boolean;
    backup_dir?: string;
    backup_frequency?: number;
    // World Config (JSON)
    seed?: string;
    world_gen_type?: string;
    world_name?: string;
    view_distance?: number;
    gameplay_config?: string;
    is_pvp_enabled?: boolean;
    is_fall_damage_enabled?: boolean;
    is_ticking?: boolean;
    is_block_ticking?: boolean;
    is_game_time_paused?: boolean;
    is_spawning_npc?: boolean;
    is_spawn_markers_enabled?: boolean;
    is_all_npc_frozen?: boolean;
    is_compass_updating?: boolean;
    is_saving_players?: boolean;
    is_saving_chunks?: boolean;
    is_unloading_chunks?: boolean;
    is_objective_markers_enabled?: boolean;
    dir_exists: boolean;
    config?: any;
    max_players?: number;
}

// JVM Args Suggestions for the config form
const JVM_ARGS_SUGGESTIONS = [
    { arg: '-XX:AOTCache=HytaleServer.aot', desc: 'Accélère considérablement le démarrage (AOT)', isRecommended: true },
    { arg: '-XX:+UseG1GC', desc: 'Garbage Collector G1', isRecommended: false },
    { arg: '-XX:+UseZGC', desc: 'Garbage Collector ZGC - Latence ultra-faible', isRecommended: false },
    { arg: '-XX:MaxGCPauseMillis=50', desc: 'Limite les pauses du GC à 50ms', isRecommended: false },
    { arg: '-XX:+ParallelRefProcEnabled', desc: 'Traite les références en parallèle', isRecommended: false },
    { arg: '-XX:+DisableExplicitGC', desc: 'Ignore les appels System.gc()', isRecommended: false },
    { arg: '-XX:+AlwaysPreTouch', desc: 'Précharge toute la RAM au démarrage', isRecommended: false },
    { arg: '-XX:+UseStringDeduplication', desc: 'Déduplique les chaînes pour économiser la RAM', isRecommended: false },
    { arg: '-Dfile.encoding=UTF-8', desc: 'Force l\'encodage UTF-8', isRecommended: false },
];

type TabId = 'console' | 'logs' | 'schedule' | 'backups' | 'files' | 'config' | 'players' | 'metrics' | 'webhooks';

interface Tab {
    id: TabId;
    label: string;
    icon: React.ReactNode;
}

const CollapsibleSection = ({ title, icon: Icon, children, badge, defaultOpen = false }: any) => (
    <details className="card collapsible-section" open={defaultOpen}>
        <summary className="collapsible-header">
            <div className="collapsible-header__content">
                <Icon size={18} className="collapsible-header__icon" />
                <span className="collapsible-header__title">{title}</span>
                {badge && (
                    <span className="collapsible-header__badge">{badge}</span>
                )}
            </div>
            <ChevronDown size={18} className="chevron-icon" />
        </summary>
        <div className="collapsible-content pt-4 border-t border-white/5 mt-3">{children}</div>
    </details>
);

export default function ServerDetail() {
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { id } = useParams<{ id: string }>();

    const [server, setServer] = useState<Server | null>(null);
    const [activeTab, setActiveTab] = useState<TabId>('console');
    const [logs, setLogs] = useState<string[]>([]);
    const [command, setCommand] = useState('');
    const [isConnected, setIsConnected] = useState(false);
    const [startTime, setStartTime] = useState<Date | null>(null);
    const [uptime, setUptime] = useState('--:--:--');
    const wsRef = useRef<WebSocket | null>(null);
    const logsEndRef = useRef<HTMLDivElement>(null);

    // Backups tab state
    const [backups, setBackups] = useState<Backup[]>([]);
    const [backupsLoading, setBackupsLoading] = useState(false);
    const [creatingBackup, setCreatingBackup] = useState(false);

    // Files tab state
    const [files, setFiles] = useState<FileEntry[]>([]);
    const [currentPath, setCurrentPath] = useState('');
    const [filesLoading, setFilesLoading] = useState(false);
    const [selectedFile, setSelectedFile] = useState<string | null>(null);
    const [fileContent, setFileContent] = useState('');
    const [fileSaving, setFileSaving] = useState(false);

    // Logs tab state
    const [logFiles, setLogFiles] = useState<FileEntry[]>([]);
    const [selectedLogFile, setSelectedLogFile] = useState<string | null>(null);
    const [logContent, setLogContent] = useState('');

    // Config tab state
    const [configFormData, setConfigFormData] = useState<Partial<Server>>({});
    const [configSaving, setConfigSaving] = useState(false);
    const [configError, setConfigError] = useState('');
    const [javaVersions, setJavaVersions] = useState<{ path: string; version: string }[]>([]);

    // Players tab state
    const [activePlayerTab, setActivePlayerTab] = useState<'online' | 'whitelist' | 'bans' | 'permissions'>('online');
    const [playerData, setPlayerData] = useState<any[]>([]); // Placeholder for list data
    const [isPlayerLoading, setIsPlayerLoading] = useState(false);

    const tabs: Tab[] = [
        { id: 'console', label: 'Terminal', icon: <Terminal size={18} /> },
        { id: 'logs', label: 'Logs', icon: <FileText size={18} /> },
        { id: 'schedule', label: 'Schedule', icon: <Calendar size={18} /> },
        { id: 'backups', label: 'Backups', icon: <History size={18} /> },
        { id: 'files', label: 'Files', icon: <FolderOpen size={18} /> },
        { id: 'config', label: 'Config', icon: <Settings size={18} /> },
        { id: 'players', label: 'Players', icon: <Users size={18} /> }, // Shortened label
        { id: 'metrics', label: 'Metrics', icon: <BarChart3 size={18} /> },
        { id: 'webhooks', label: 'Webhooks', icon: <Webhook size={18} /> },
    ];

    useEffect(() => {
        // Reset logs on id change
        setLogs([]);
        fetchServer();
        // Try to fetch existing install logs to restore history
        fetchInstallLog();
        connectWebSocket();

        return () => {
            wsRef.current?.close();
        };
    }, [id]);

    const fetchInstallLog = async () => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=server/logs/install.log`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            if (response.ok) {
                const data = await response.json();
                if (data.content && data.content.length > 0) {
                    const lines = data.content.split('\n');
                    setLogs(prev => {
                        // If we already have logs (from WS), prepend file content? 
                        // Usually this runs fast on mount. 
                        // We deduplicate simple assumption: if file has content, it's history.
                        return [...lines, ...prev];
                    });
                }
            }
        } catch (e) {
            // File might not exist if not installing, ignore
        }
    };

    useEffect(() => {
        logsEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }, [logs]);

    useEffect(() => {
        if (server) {
            setPageTitle(server.name, 'Hytale Server', { to: '/servers' });
        } else {
            setPageTitle(t('common.loading'), '', { to: '/servers' });
        }
    }, [server, setPageTitle, t]);

    useEffect(() => {
        if (server?.status === 'running' && startTime) {
            const interval = setInterval(() => {
                const diff = Date.now() - startTime.getTime();
                const hours = Math.floor(diff / 3600000);
                const minutes = Math.floor((diff % 3600000) / 60000);
                const seconds = Math.floor((diff % 60000) / 1000);
                setUptime(`${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`);
            }, 1000);
            return () => clearInterval(interval);
        } else {
            setUptime('--:--:--');
        }
    }, [server?.status, startTime]);

    // Config form logic
    useEffect(() => {
        if (activeTab === 'config') {
            fetchJavaVersions();
            // Initialize form data from server and its config
            if (server && configFormData.id !== server.id) {
                const formData = { ...server };
                // Map config values to flat form fields if they exist
                if (server.config) {
                    if (server.config.MaxPlayers) formData.max_players = server.config.MaxPlayers;
                    if (server.config.MaxViewRadius) formData.view_distance = server.config.MaxViewRadius;
                    if (server.config.Seed) formData.seed = server.config.Seed;
                    if (server.config.ServerName) formData.name = server.config.ServerName;
                }
                setConfigFormData(formData);
            }
        }
    }, [activeTab, server, configFormData.id]);

    const fetchJavaVersions = async () => {
        try {
            const response = await fetch('/api/v1/system/java-versions', {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` }
            });
            if (response.ok) {
                setJavaVersions(await response.json());
            }
        } catch (error) {
            console.error('Failed to fetch Java versions:', error);
        }
    };

    const updateConfigValue = <K extends keyof Server>(key: K, value: Server[K]) => {
        setConfigFormData(prev => ({ ...prev, [key]: value }));
    };

    const toggleJvmArg = (arg: string) => {
        let currentArgs = configFormData.extra_args || '';
        let currentArgsParts = currentArgs.trim().split(/\s+/).filter(a => a.length > 0);

        if (currentArgsParts.includes(arg)) {
            currentArgsParts = currentArgsParts.filter(a => a !== arg);
        } else {
            currentArgsParts.push(arg);
        }

        updateConfigValue('extra_args', currentArgsParts.join(' '));
    };

    const handleSaveConfig = async (e: React.FormEvent) => {
        e.preventDefault();
        configFormData.id = id; // Ensure ID is set
        setConfigSaving(true);
        setConfigError('');

        try {
            // Prepare payload: merge flat fields back into config object where necessary
            const payload = { ...configFormData };

            // Ensure config object exists
            if (!payload.config) payload.config = server?.config || {};

            // Update specific config fields
            if (payload.max_players) payload.config.MaxPlayers = parseInt(payload.max_players.toString());
            if (payload.view_distance) payload.config.MaxViewRadius = parseInt(payload.view_distance.toString());
            if (payload.seed) payload.config.Seed = payload.seed;
            // config.ServerName is usually handled by the main 'name' field update in backend? 
            // The backend updates 'name' column, but we should also sync it to config if Hytale needs it there.
            if (payload.name) payload.config.ServerName = payload.name;


            const response = await fetch(`/api/v1/servers/${id}`, {
                method: 'PUT',
                headers: {
                    'Content-Type': 'application/json',
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify(payload),
            });

            if (response.ok) {
                // Refresh server data
                fetchServer();
                alert('Configuration sauvegardée !');
            } else {
                const data = await response.json();
                setConfigError(data.error || 'Erreur lors de la sauvegarde');
            }
        } catch (err) {
            setConfigError('Erreur de connexion');
        } finally {
            setConfigSaving(false);
        }
    };

    const fetchServer = async () => {
        const response = await fetch(`/api/v1/servers/${id}`, {
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
        });
        const data = await response.json();
        setServer(data);
        if (data.status === 'running' && !startTime) {
            setStartTime(new Date());
        } else if (data.status !== 'running') {
            setStartTime(null);
        }
    };

    const connectWebSocket = () => {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const ws = new WebSocket(`${protocol}//${window.location.host}/ws/console/${id}`);

        ws.onopen = () => {
            setIsConnected(true);
        };

        ws.onmessage = (event) => {
            setLogs((prev) => [...prev, event.data]);
        };

        ws.onclose = () => {
            setIsConnected(false);
        };

        wsRef.current = ws;
    };

    const handleAction = async (action: 'start' | 'stop' | 'restart' | 'kill') => {
        await fetch(`/api/v1/servers/${id}/${action}`, {
            method: 'POST',
            headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
        });
        if (action === 'start') {
            setStartTime(new Date());
        } else if (action === 'stop' || action === 'kill') {
            setStartTime(null);
        }
        fetchServer();
        setTimeout(fetchServer, 1000);
        setTimeout(fetchServer, 3000);
    };

    const handleReinstall = async () => {
        if (!confirm("Êtes-vous sûr de vouloir réinstaller ce serveur ? Cette action supprimera les fichiers du serveur et en téléchargera de nouveaux. Les mondes et configurations seront conservés si possible, mais c'est risqué.")) {
            return;
        }

        try {
            const response = await fetch(`/api/v1/servers/${id}/reinstall`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });

            if (response.ok) {
                alert("Réinstallation lancée. Le serveur va être arrêté et réinstallé.");
                fetchServer();
            } else {
                alert("Erreur lors du lancement de la réinstallation.");
            }
        } catch (e) {
            console.error(e);
            alert("Erreur de connexion.");
        }
    };

    const handleDelete = async () => {
        const confirmName = prompt(`Pour confirmer la suppression, tapez "${server?.name}" :`);
        if (confirmName !== server?.name) {
            if (confirmName) alert("Nom incorrect, suppression annulée.");
            return;
        }

        try {
            const response = await fetch(`/api/v1/servers/${id}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });

            if (response.ok) {
                // Redirect to servers list
                window.location.href = '/servers'; // Simple redirect since we don't have navigate hook setup right here in this view (or we can add it)
            } else {
                alert("Erreur lors de la suppression.");
            }
        } catch (e) {
            console.error(e);
            alert("Erreur de connexion.");
        }
    };

    const sendCommand = (e: React.FormEvent) => {
        e.preventDefault();
        if (command.trim() && wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(command);
            setLogs((prev) => [...prev, `> ${command}`]);
            setCommand('');
        }
    };

    // Backup functions
    const fetchBackups = useCallback(async () => {
        if (!id) return;
        setBackupsLoading(true);
        try {
            const response = await fetch(`/api/v1/backups?server_id=${id}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            const data = await response.json();
            setBackups(data);
        } catch (error) {
            console.error('Failed to fetch backups:', error);
        } finally {
            setBackupsLoading(false);
        }
    }, [id]);

    const createBackup = async () => {
        if (!id) return;
        setCreatingBackup(true);
        try {
            await fetch('/api/v1/backups', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify({ server_id: id }),
            });
            fetchBackups();
        } catch (error) {
            console.error('Failed to create backup:', error);
        } finally {
            setCreatingBackup(false);
        }
    };

    const deleteBackup = async (backupId: string) => {
        if (!confirm('Supprimer ce backup ?')) return;
        try {
            await fetch(`/api/v1/backups/${backupId}`, {
                method: 'DELETE',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            fetchBackups();
        } catch (error) {
            console.error('Failed to delete backup:', error);
        }
    };

    const restoreBackup = async (backupId: string) => {
        if (!confirm('Restaurer ce backup ? Les données actuelles seront écrasées.')) return;
        try {
            await fetch(`/api/v1/backups/${backupId}/restore`, {
                method: 'POST',
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            alert('Backup restauré avec succès !');
        } catch (error) {
            console.error('Failed to restore backup:', error);
        }
    };

    const formatBytes = (bytes: number) => {
        if (bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    };

    // Files functions
    const fetchFiles = useCallback(async (path = '') => {
        if (!id) return;
        setFilesLoading(true);
        try {
            const response = await fetch(`/api/v1/servers/${id}/files?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            const data = await response.json();
            setFiles(data.entries || []);
            setCurrentPath(data.current_path || '');
        } catch (error) {
            console.error('Failed to fetch files:', error);
        } finally {
            setFilesLoading(false);
        }
    }, [id]);

    const readFile = async (path: string) => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            const data = await response.json();
            setFileContent(data.content || '');
            setSelectedFile(path);
        } catch (error) {
            console.error('Failed to read file:', error);
        }
    };

    const saveFile = async () => {
        if (!id || !selectedFile) return;
        setFileSaving(true);
        try {
            await fetch(`/api/v1/servers/${id}/files/write`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify({ path: selectedFile, content: fileContent }),
            });
            alert('Fichier sauvegardé !');
        } catch (error) {
            console.error('Failed to save file:', error);
        } finally {
            setFileSaving(false);
        }
    };

    // Fetch backups/files when tab changes
    useEffect(() => {
        if (activeTab === 'backups') {
            fetchBackups();
        } else if (activeTab === 'files') {
            fetchFiles();
            setSelectedFile(null);
            setFileContent('');
        } else if (activeTab === 'logs') {
            fetchLogFiles();
        }
    }, [activeTab, fetchBackups, fetchFiles]);

    // Logs functions
    const fetchLogFiles = async () => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files?path=server/logs`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            const data = await response.json();
            const logs = (data.entries || []).filter((f: FileEntry) => !f.is_dir);
            setLogFiles(logs);
            // Auto-select first log file
            if (logs.length > 0 && !selectedLogFile) {
                readLogFile(logs[0].path);
            }
        } catch (error) {
            console.error('Failed to fetch log files:', error);
        }
    };

    const readLogFile = async (path: string) => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            const data = await response.json();
            setLogContent(data.content || '');
            setSelectedLogFile(path);
        } catch (error) {
            console.error('Failed to read log file:', error);
        }
    };

    // Players Data Logic
    const fetchPlayerData = useCallback(async () => {
        if (!id || activePlayerTab === 'online') return;

        setIsPlayerLoading(true);
        let filename = '';
        if (activePlayerTab === 'whitelist') filename = 'server/whitelist.json';
        else if (activePlayerTab === 'bans') filename = 'server/bans.json';
        else if (activePlayerTab === 'permissions') filename = 'server/permissions.json';

        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${filename}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem('token')}` },
            });
            if (response.ok) {
                const data = await response.json();
                try {
                    const parsed = JSON.parse(data.content);
                    setPlayerData(Array.isArray(parsed) ? parsed : []);
                } catch (e) {
                    console.warn('Failed to parse player file, strictly not an array or invalid JSON', e);
                    setPlayerData([]);
                }
            } else {
                setPlayerData([]); // File might not exist yet
            }
        } catch (error) {
            console.error('Failed to fetch player data:', error);
            setPlayerData([]);
        } finally {
            setIsPlayerLoading(false);
        }
    }, [id, activePlayerTab]);

    const savePlayerData = async (newData: any[]) => {
        if (!id) return;
        let filename = '';
        if (activePlayerTab === 'whitelist') filename = 'server/whitelist.json';
        else if (activePlayerTab === 'bans') filename = 'server/bans.json';
        else if (activePlayerTab === 'permissions') filename = 'server/permissions.json';

        try {
            await fetch(`/api/v1/servers/${id}/files/write`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    Authorization: `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify({
                    path: filename,
                    content: JSON.stringify(newData, null, 4)
                }),
            });
            setPlayerData(newData);
            alert('Données sauvegardées !');
        } catch (error) {
            console.error('Failed to save player data:', error);
            alert('Erreur lors de la sauvegarde.');
        }
    };

    useEffect(() => {
        if (activeTab === 'players') {
            fetchPlayerData();
        }
    }, [activeTab, fetchPlayerData]);

    if (!server) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
            </div>
        );
    }

    const isRunning = server.status === 'running';
    const isMissing = server.status === 'missing';
    // Fix: Removed server.config usage as fields are now on root or removed
    const maxPlayers = 100; // Default or fetched from metrics/query if implemented
    const port = server.port || 5520;
    const bindAddress = server.bind_address || '0.0.0.0';

    return (
        <div className="server-detail-page">

            {/* Server Info Block */}
            <div className="server-stats">
                {/* Status */}
                <div className="stat-card">
                    <div className={`stat-card__icon ${isRunning ? 'stat-card__icon--online' : isMissing ? 'stat-card__icon--warning' : 'stat-card__icon--offline'}`}>
                        <Globe size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Status</div>
                        <div className={`stat-card__value ${isRunning ? 'stat-card__value--success' : isMissing ? 'stat-card__value--warning' : 'stat-card__value--danger'}`}>
                            {isRunning ? 'Online' : isMissing ? 'Missing' : 'Offline'}
                        </div>
                    </div>
                </div>

                {/* Uptime */}
                <div className="stat-card">
                    <div className="stat-card__icon">
                        <Clock size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Temps de fonctionnement</div>
                        <div className="stat-card__value stat-card__value--mono">{uptime}</div>
                    </div>
                </div>

                {/* CPU */}
                <div className="stat-card">
                    <div className="stat-card__icon">
                        <Cpu size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Processeur (CPU)</div>
                        <div className="stat-card__value">{isRunning ? '--' : '0'}%</div>
                    </div>
                </div>

                {/* Memory */}
                <div className="stat-card">
                    <div className="stat-card__icon">
                        <HardDrive size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Mémoire (RAM)</div>
                        <div className="stat-card__value">{isRunning ? '--' : '0'} / {server.max_memory || '4G'}</div>
                    </div>
                </div>

                {/* Players */}
                <div className="stat-card">
                    <div className="stat-card__icon">
                        <Users size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Joueurs</div>
                        <div className="stat-card__value">0 / {maxPlayers}</div>
                    </div>
                </div>

                {/* Address */}
                <div className="stat-card">
                    <div className="stat-card__icon">
                        <Terminal size={18} />
                    </div>
                    <div className="stat-card__content">
                        <div className="stat-card__label">Adresse IP</div>
                        <div className="stat-card__value stat-card__value--mono">{bindAddress}:{port}</div>
                    </div>
                </div>
            </div>

            {/* Tabs - Navbar Style */}
            <div className="server-tabs">
                {tabs.map(tab => (
                    <button
                        key={tab.id}
                        onClick={() => setActiveTab(tab.id)}
                        className={`tab-btn ${activeTab === tab.id ? 'tab-btn--active' : ''}`}
                    >
                        {tab.icon}
                        {tab.label}
                    </button>
                ))}
            </div>

            {/* Tab Content */}
            <div className="tab-content">
                {activeTab === 'console' && (
                    <div className="card">
                        <div className="panel-header">
                            <h3 className="panel-header__title"><Terminal size={20} /> Terminal</h3>
                            <div className={`status-badge ${isConnected ? 'status-badge--success' : 'status-badge--error'}`}>
                                {isConnected ? 'Connecté' : 'Déconnecté'}
                            </div>
                        </div>
                        <div className="console-container">

                            {/* Console Viewport */}
                            <div className="console-output">
                                {logs.length === 0 ? (
                                    <div className="console-output__empty">
                                        <Terminal size={36} />
                                        <span>
                                            {isRunning ? 'En attente des logs...' : 'Le serveur est hors ligne.'}
                                        </span>
                                    </div>
                                ) : (
                                    logs.map((log, i) => (
                                        <div
                                            key={i}
                                            className={`console-output__line ${log.includes('[ERROR]') || log.includes('ERROR') || log.includes('Exception')
                                                ? 'console-output__line--error'
                                                : log.includes('[WARN]') || log.includes('WARN')
                                                    ? 'console-output__line--warning'
                                                    : log.includes('[INFO]')
                                                        ? 'console-output__line--info'
                                                        : log.startsWith('>')
                                                            ? 'console-output__line--command'
                                                            : ''
                                                }`}
                                        >
                                            {log}
                                        </div>
                                    ))
                                )}
                                <div ref={logsEndRef} />
                            </div>

                            {/* Command Input Area */}
                            <form onSubmit={sendCommand} className="command-form">
                                <input
                                    type="text"
                                    value={command}
                                    onChange={(e) => setCommand(e.target.value)}
                                    placeholder="Enter your command..."
                                    disabled={!isConnected || !isRunning}
                                    className="form-input"
                                />
                                <button
                                    type="submit"
                                    className="btn btn--primary"
                                    disabled={!isConnected || !isRunning}
                                >
                                    Send
                                </button>
                            </form>

                            {/* Action Buttons */}
                            <div className="action-buttons">
                                <button
                                    className="btn btn--primary btn--lg"
                                    onClick={() => handleAction('start')}
                                    disabled={isMissing || isRunning}
                                >
                                    Start
                                </button>

                                <button
                                    className="btn btn--secondary btn--lg"
                                    onClick={() => handleAction('restart')}
                                    disabled={!isRunning}
                                >
                                    Restart
                                </button>

                                <button
                                    className="btn btn--danger-solid btn--lg"
                                    onClick={() => handleAction('stop')}
                                    disabled={!isRunning}
                                >
                                    Stop
                                </button>
                            </div>
                        </div>
                    </div>
                )}

                {activeTab === 'backups' && (
                    <div>
                        {/* Header */}
                        <div className="panel-header">
                            <h3 className="panel-header__title"><History size={20} /> Sauvegardes</h3>
                            <button
                                onClick={createBackup}
                                disabled={creatingBackup}
                                className="btn btn--primary btn--sm"
                            >
                                <Plus size={16} />
                                {creatingBackup ? 'Création...' : 'Nouveau backup'}
                            </button>
                        </div>

                        {/* Content */}
                        <div className="list-container">
                            {backupsLoading ? (
                                <div className="loading-screen relative">
                                    <div className="spinner"></div>
                                </div>
                            ) : backups.length === 0 ? (
                                <div className="empty-state">
                                    <History size={48} className="empty-state-icon" />
                                    <p className="font-medium">Aucune sauvegarde</p>
                                    <p className="text-sm text-muted">Créez votre première sauvegarde pour protéger vos données.</p>
                                </div>
                            ) : (
                                backups.map((backup) => (
                                    <div key={backup.id} className="list-item">
                                        <div className="list-item__info">
                                            <History size={20} className="text-muted" />
                                            <div className="list-item__details">
                                                <div className="list-item__name">{backup.filename}</div>
                                                <div className="list-item__meta">
                                                    {formatBytes(backup.size_bytes)} • {new Date(backup.created_at).toLocaleString('fr-FR')}
                                                </div>
                                            </div>
                                        </div>
                                        <div className="list-item__actions">
                                            <button
                                                onClick={() => restoreBackup(backup.id)}
                                                title="Restaurer"
                                                className="btn btn--icon btn--ghost"
                                            >
                                                <Download size={16} />
                                            </button>
                                            <button
                                                onClick={() => deleteBackup(backup.id)}
                                                title="Supprimer"
                                                className="btn btn--icon btn--ghost btn--danger"
                                            >
                                                <Trash2 size={16} />
                                            </button>
                                        </div>
                                    </div>
                                ))
                            )}
                        </div>
                    </div>
                )}

                {activeTab === 'files' && (
                    <div className="card">
                        {/* Breadcrumb & Quick Links */}
                        <div className="panel-header flex-col items-start gap-4">
                            <div className="flex items-center gap-2 w-full justify-between">
                                <div className="panel-header__title">
                                    <FolderOpen size={20} />
                                    <span className="panel-header__path">
                                        /{server.name} ({server.id}){currentPath ? `/${currentPath}` : ''}
                                    </span>
                                </div>
                                <div className="flex gap-2">
                                    <button onClick={() => fetchFiles('')} className="btn btn--sm btn--ghost" title="Racine">
                                        <Folder size={14} /> /
                                    </button>
                                    <button onClick={() => fetchFiles('mods')} className="btn btn--sm btn--ghost" title="Mods">
                                        Mods
                                    </button>
                                    <button onClick={() => fetchFiles('universe')} className="btn btn--sm btn--ghost" title="Mondes (Universe)">
                                        Universe
                                    </button>
                                    <button onClick={() => fetchFiles('server/logs')} className="btn btn--sm btn--ghost" title="Logs">
                                        Logs
                                    </button>
                                </div>
                            </div>
                        </div>

                        {/* Two column layout */}
                        <div className={`file-manager ${selectedFile ? 'file-manager--with-editor' : ''}`}>
                            {/* File list */}
                            <div className="file-manager__list">
                                {filesLoading ? (
                                    <div className="loading-screen relative">
                                        <div className="spinner"></div>
                                    </div>
                                ) : files.length === 0 ? (
                                    <div className="empty-state">
                                        <FolderOpen size={32} style={{ opacity: 0.3 }} />
                                        <p>Dossier vide</p>
                                    </div>
                                ) : (
                                    <div className="list-container">
                                        {files.map((file) => (
                                            <div
                                                key={file.path}
                                                onClick={() => {
                                                    if (file.is_dir) {
                                                        fetchFiles(file.path);
                                                        setSelectedFile(null);
                                                        setFileContent('');
                                                    } else {
                                                        readFile(file.path);
                                                    }
                                                }}
                                                className={`list-item cursor-pointer ${selectedFile === file.path ? 'list-item--selected' : ''}`}
                                            >
                                                <div className="list-item__info">
                                                    {file.is_dir ? (
                                                        <Folder size={16} className="list-item__icon--folder" />
                                                    ) : (
                                                        <File size={16} className="list-item__icon--file" />
                                                    )}
                                                    <span className="list-item__name">{file.name}</span>
                                                </div>
                                                <div className="file-item-meta">
                                                    {!file.is_dir && file.size !== undefined && (
                                                        <span className="list-item__meta">
                                                            {formatBytes(file.size)}
                                                        </span>
                                                    )}
                                                    {file.is_dir && <ChevronRight size={14} className="text-muted" />}
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </div>

                            {/* File editor */}
                            {selectedFile && (
                                <div className="file-manager__editor">
                                    <div className="file-manager__editor-toolbar">
                                        <span className="panel-header__path">
                                            {selectedFile}
                                        </span>
                                        <button
                                            onClick={saveFile}
                                            disabled={fileSaving}
                                            className="btn btn--primary btn--sm"
                                        >
                                            <Save size={14} />
                                            {fileSaving ? 'Saving...' : 'Save'}
                                        </button>
                                    </div>
                                    <textarea
                                        value={fileContent}
                                        onChange={(e) => setFileContent(e.target.value)}
                                        spellCheck={false}
                                    />
                                </div>
                            )}
                        </div>
                    </div>
                )}

                {activeTab === 'logs' && (
                    <div className="card">
                        {/* Header */}
                        <div className="panel-header">
                            <h3 className="panel-header__title"><FileText size={20} /> Fichiers de log</h3>
                            {logFiles.length > 0 && (
                                <div className="select-wrapper">
                                    <select
                                        value={selectedLogFile || ''}
                                        onChange={(e) => readLogFile(e.target.value)}
                                        className="form-select text-sm py-1"
                                    >
                                        {logFiles.map(f => (
                                            <option key={f.path} value={f.path}>{f.name}</option>
                                        ))}
                                    </select>
                                </div>
                            )}
                        </div>

                        {/* Log content */}
                        <div className="console-container console-logs-file">
                            <div className="console-output">
                                {logFiles.length === 0 ? (
                                    <div className="console-output__empty">
                                        <AlertCircle size={32} />
                                        <p className="font-medium">Aucun fichier de log trouvé</p>
                                        <p className="text-sm">
                                            Le dossier <code className="bg-dark px-1 rounded">server/logs</code> est vide ou n'existe pas.
                                        </p>
                                    </div>
                                ) : (
                                    <pre className="log-content-pre">
                                        {logContent || 'Chargement... ou fichier vide.'}
                                    </pre>
                                )}
                            </div>
                        </div>
                    </div>
                )}

                {activeTab === 'config' && (
                    <div className="card">
                        <div className="panel-header">
                            <h3 className="panel-header__title"><Settings size={20} /> Configuration</h3>
                        </div>
                        <div className="config-tab-content">
                            <form onSubmit={handleSaveConfig}>
                                <div className="content-container">

                                    {/* General (Manager Only) */}
                                    <div className="card form-section p-0 border-0 shadow-none bg-transparent">
                                        <h3 className="form-section-title">
                                            <ServerIcon size={18} />
                                            Informations Générales (Manager)
                                        </h3>
                                        <div className="form-column">
                                            <div className="form-group">
                                                <label>Nom du serveur (Interne)</label>
                                                <input
                                                    type="text"
                                                    value={configFormData.name || ''}
                                                    onChange={(e) => updateConfigValue('name', e.target.value)}
                                                    className="input"
                                                    required
                                                    placeholder="Mon Serveur"
                                                />
                                            </div>
                                            <div className="form-grid-2">
                                                <div className="form-group">
                                                    <label>Répertoire de travail</label>
                                                    <input type="text" value={configFormData.working_dir || ''} onChange={(e) => updateConfigValue('working_dir', e.target.value)} className="input font-mono" />
                                                </div>
                                            </div>
                                        </div>
                                    </div>

                                    {/* Launch Arguments (CLI) */}
                                    <div className="card form-section p-0 border-0 shadow-none bg-transparent mt-6">
                                        <h3 className="form-section-title">
                                            <Terminal size={18} />
                                            Arguments de Lancement (CLI)
                                        </h3>
                                        <div className="form-grid-2">
                                            <div className="form-group">
                                                <label>Exécutable (JAR)</label>
                                                <input type="text" value={configFormData.executable_path || ''} onChange={(e) => updateConfigValue('executable_path', e.target.value)} className="input font-mono" />
                                                <p className="helper-text">ex: <code className="bg-dark px-1 rounded">HytaleServer.jar</code></p>
                                            </div>
                                            <div className="form-group">
                                                <label>Assets (ZIP)</label>
                                                <input type="text" value={configFormData.assets_path || ''} onChange={(e) => updateConfigValue('assets_path', e.target.value)} className="input font-mono" />
                                                <p className="helper-text">--assets &lt;Path&gt;</p>
                                            </div>
                                            <div className="form-group">
                                                <label>Adresse IP (--bind)</label>
                                                <input type="text" value={configFormData.bind_address || '0.0.0.0'} onChange={(e) => updateConfigValue('bind_address', e.target.value)} className="input" />
                                            </div>
                                            <div className="form-group">
                                                <label>Port UDP (QUIC)</label>
                                                <input type="number" value={configFormData.port || 5520} onChange={(e) => updateConfigValue('port', parseInt(e.target.value))} className="input" />
                                            </div>
                                            <div className="form-group">
                                                <label>Mode d'Authentification</label>
                                                <Select
                                                    options={[{ label: 'Authenticated', value: 'authenticated' }, { label: 'Offline', value: 'offline' }]}
                                                    value={configFormData.auth_mode || 'authenticated'}
                                                    onChange={(v) => updateConfigValue('auth_mode', v as any)}
                                                />
                                            </div>
                                            <div className="checkbox-group full-width">
                                                <Checkbox
                                                    checked={configFormData.allow_op || false}
                                                    onChange={(v) => updateConfigValue('allow_op', v)}
                                                    label="Autoriser les opérateurs (--allow-op)"
                                                    description="Permet aux administrateurs désignés d'utiliser les commandes sensibles."
                                                />
                                            </div>
                                            <div className="checkbox-group full-width">
                                                <Checkbox
                                                    checked={configFormData.disable_sentry || false}
                                                    onChange={(v) => updateConfigValue('disable_sentry', v)}
                                                    label="Désactiver Sentry (--disable-sentry)"
                                                    description="Désactive l'envoi de rapports d'erreurs automatique (recommandé pour le dev)."
                                                />
                                            </div>
                                            <div className="checkbox-group full-width">
                                                <Checkbox
                                                    checked={configFormData.accept_early_plugins || false}
                                                    onChange={(v) => updateConfigValue('accept_early_plugins', v)}
                                                    label="Accepter les early-plugins"
                                                    description="Autorise le chargement de plugins instables ou en développement."
                                                />
                                            </div>
                                        </div>
                                    </div>

                                    {/* Resources (JVM) */}
                                    <CollapsibleSection title="Ressources (JVM)" icon={Cpu} defaultOpen={true}>
                                        <div className="form-grid-2">
                                            <div className="form-group">
                                                <label>RAM Minimale (-Xms)</label>
                                                <input type="text" value={configFormData.min_memory || ''} onChange={(e) => updateConfigValue('min_memory', e.target.value)} className="input" placeholder="ex: 1G" />
                                            </div>
                                            <div className="form-group">
                                                <label>RAM Maximale (-Xmx)</label>
                                                <input type="text" value={configFormData.max_memory || ''} onChange={(e) => updateConfigValue('max_memory', e.target.value)} className="input" placeholder="ex: 4G" />
                                            </div>
                                            <div className="form-group full-width">
                                                <label>Chemin Java</label>
                                                <Select
                                                    options={[{ label: 'Défaut Système', value: '' }, ...javaVersions.map(j => ({ label: `Java ${j.version} (${j.path})`, value: j.path }))]}
                                                    value={configFormData.java_path || ''}
                                                    onChange={(v) => updateConfigValue('java_path', v)}
                                                />
                                            </div>
                                            <div className="form-group full-width">
                                                <label>Arguments JVM</label>
                                                <input type="text" value={configFormData.extra_args || ''} onChange={(e) => updateConfigValue('extra_args', e.target.value)} className="input font-mono" />
                                                <div className="jvm-args-shortcuts flex flex-col gap-2 mt-3">
                                                    {JVM_ARGS_SUGGESTIONS.map(({ arg, desc, isRecommended }) => (
                                                        <Checkbox
                                                            key={arg}
                                                            checked={configFormData.extra_args?.includes(arg) || false}
                                                            onChange={() => toggleJvmArg(arg)}
                                                            label={
                                                                <span className="font-mono text-sm">
                                                                    {arg}
                                                                    {isRecommended && <span className="ml-2 text-xs bg-primary/20 text-primary px-1.5 py-0.5 rounded">Recommandé</span>}
                                                                </span>
                                                            }
                                                            description={desc}
                                                        />
                                                    ))}
                                                </div>
                                            </div>
                                        </div>
                                    </CollapsibleSection>

                                    {/* World Configuration (JSON) */}
                                    <CollapsibleSection title="Configuration du Monde (JSON)" icon={Globe}>
                                        <div className="form-grid-2">
                                            <div className="form-group">
                                                <label>Joueurs Maximum</label>
                                                <input
                                                    type="number"
                                                    value={configFormData.max_players || 100}
                                                    onChange={(e) => updateConfigValue('max_players', parseInt(e.target.value))}
                                                    className="input"
                                                />
                                            </div>
                                            <div className="form-group">
                                                <label>Seed</label>
                                                <input type="text" value={configFormData.seed || ''} onChange={(e) => updateConfigValue('seed', e.target.value)} className="input font-mono" />
                                                <p className="helper-text">Graine de génération du monde. Laisser vide pour une génération aléatoire.</p>
                                            </div>
                                            <div className="form-group">
                                                <label>Distance de vue (Chunks)</label>
                                                <RangeSlider
                                                    min={4}
                                                    max={32}
                                                    value={configFormData.view_distance || 12}
                                                    onChange={(v) => updateConfigValue('view_distance', v)}
                                                />
                                                <p className="helper-text">
                                                    {(configFormData.view_distance || 12) * 32} blocs. Hytale recommande 12 chunks (384 blocs) max pour la performance.
                                                </p>
                                            </div>
                                            <div className="form-group">
                                                <label>Type de Génération</label>
                                                <Select
                                                    options={[{ label: 'Hytale', value: 'Hytale' }, { label: 'Flat', value: 'Flat' }]}
                                                    value={configFormData.world_gen_type || 'Hytale'}
                                                    onChange={(v) => updateConfigValue('world_gen_type', v)}
                                                />
                                            </div>
                                            <div className="form-group">
                                                <label>Nom de la Génération</label>
                                                <input type="text" value={configFormData.world_name || 'Default'} onChange={(e) => updateConfigValue('world_name', e.target.value)} className="input" />
                                            </div>

                                            {/* Booleans Grid */}
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_pvp_enabled !== false}
                                                    onChange={(v) => updateConfigValue('is_pvp_enabled', v)}
                                                    label="PvP Enabled"
                                                    description="Active le combat entre joueurs sur le serveur."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_fall_damage_enabled !== false}
                                                    onChange={(v) => updateConfigValue('is_fall_damage_enabled', v)}
                                                    label="Fall Damage"
                                                    description="Les joueurs subissent des dégâts de chute."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_ticking !== false}
                                                    onChange={(v) => updateConfigValue('is_ticking', v)}
                                                    label="Is Ticking"
                                                    description="Active la boucle de jeu principale (physique, IA, etc.)."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_block_ticking !== false}
                                                    onChange={(v) => updateConfigValue('is_block_ticking', v)}
                                                    label="Is Block Ticking"
                                                    description="Active les mises à jour aléatoires des blocs (feu, plantes)."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_game_time_paused !== true}
                                                    onChange={(v) => updateConfigValue('is_game_time_paused', v)}
                                                    label="Is Game Time Paused"
                                                    description="Met en pause le cycle jour/nuit."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_spawning_npc !== false}
                                                    onChange={(v) => updateConfigValue('is_spawning_npc', v)}
                                                    label="Spawning NPC"
                                                    description="Autorise l'apparition de nouvelles créatures et PNJs."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_spawn_markers_enabled !== false}
                                                    onChange={(v) => updateConfigValue('is_spawn_markers_enabled', v)}
                                                    label="Spawn Markers"
                                                    description="Utilise les points d'apparition définis dans la carte."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_all_npc_frozen !== true}
                                                    onChange={(v) => updateConfigValue('is_all_npc_frozen', v)}
                                                    label="Freeze All NPCs"
                                                    description="Fige l'IA et les mouvements de tous les PNJs."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_compass_updating !== false}
                                                    onChange={(v) => updateConfigValue('is_compass_updating', v)}
                                                    label="Compass Updating"
                                                    description="Met à jour la boussole des joueurs."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_saving_players !== false}
                                                    onChange={(v) => updateConfigValue('is_saving_players', v)}
                                                    label="Save Players"
                                                    description="Sauvegarde les données des joueurs (inventaire, pos)."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_saving_chunks !== false}
                                                    onChange={(v) => updateConfigValue('is_saving_chunks', v)}
                                                    label="Save Chunks"
                                                    description="Sauvegarde les modifications du monde sur le disque."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_unloading_chunks !== false}
                                                    onChange={(v) => updateConfigValue('is_unloading_chunks', v)}
                                                    label="Unload Chunks"
                                                    description="Décharge les chunks inutilisés pour libérer la RAM."
                                                />
                                            </div>
                                            <div className="checkbox-group mt-2">
                                                <Checkbox
                                                    checked={configFormData.is_objective_markers_enabled !== false}
                                                    onChange={(v) => updateConfigValue('is_objective_markers_enabled', v)}
                                                    label="Objective Markers"
                                                    description="Affiche les marqueurs d'objectifs en jeu."
                                                />
                                            </div>
                                        </div>
                                    </CollapsibleSection>



                                    {configError && <div className="alert alert--error">{configError}</div>}

                                    <div className="action-bar">
                                        <button type="submit" className="btn btn--primary" disabled={configSaving}>
                                            <Save size={18} />
                                            {configSaving ? 'Sauvegarde...' : 'Enregistrer les modifications'}
                                        </button>
                                    </div>
                                </div>
                            </form>

                            {/* Danger Zone */}
                            <div className="card form-section p-0 border-0 shadow-none bg-transparent mt-8 border-t border-danger/20 pt-6">
                                <h3 className="form-section-title text-danger">
                                    <AlertCircle size={18} />
                                    Zone de Danger
                                </h3>
                                <div className="danger-zone-grid grid grid-cols-1 md:grid-cols-2 gap-4">
                                    <div className="danger-item p-4 border border-white/5 rounded bg-white/5">
                                        <h4 className="font-semibold mb-2">Réinstaller le serveur</h4>
                                        <p className="text-sm text-muted mb-4">
                                            Supprime et télécharge à nouveau les fichiers du serveur. Vos données (mondes, backups) devraient être préservées, mais une sauvegarde est recommandée.
                                        </p>
                                        <button
                                            type="button"
                                            onClick={handleReinstall}
                                            className="btn btn--secondary w-full"
                                        >
                                            <Download size={16} /> Réinstaller
                                        </button>
                                    </div>
                                    <div className="danger-item p-4 border border-danger/20 rounded bg-danger/5">
                                        <h4 className="font-semibold mb-2 text-danger">Supprimer le serveur</h4>
                                        <p className="text-sm text-muted mb-4">
                                            Cette action est irréversible. Toutes les données, fichiers et sauvegardes seront définitivement effacés.
                                        </p>
                                        <button
                                            type="button"
                                            onClick={handleDelete}
                                            className="btn btn--danger w-full"
                                        >
                                            <Trash2 size={16} /> Supprimer
                                        </button>
                                    </div>
                                </div>
                            </div>
                        </div >
                    </div >
                )
                }

                {
                    activeTab === 'players' && (
                        <div className="card">
                            <div className="panel-header">
                                <h3 className="panel-header__title"><Users size={20} /> Gestion des Joueurs</h3>
                            </div>
                            <div className="players-tab-container">
                                {/* Sub-nav */}
                                <div className="server-tabs server-tabs--sub">
                                    <button
                                        onClick={() => setActivePlayerTab('online')}
                                        className={`tab-btn ${activePlayerTab === 'online' ? 'tab-btn--active' : ''}`}
                                    >
                                        <Globe size={16} /> En ligne
                                    </button>
                                    <button
                                        onClick={() => setActivePlayerTab('whitelist')}
                                        className={`tab-btn ${activePlayerTab === 'whitelist' ? 'tab-btn--active' : ''}`}
                                    >
                                        <Check size={16} /> Whitelist
                                    </button>
                                    <button
                                        onClick={() => setActivePlayerTab('bans')}
                                        className={`tab-btn ${activePlayerTab === 'bans' ? 'tab-btn--active' : ''}`}
                                    >
                                        <AlertCircle size={16} /> Bannissements
                                    </button>
                                    <button
                                        onClick={() => setActivePlayerTab('permissions')}
                                        className={`tab-btn ${activePlayerTab === 'permissions' ? 'tab-btn--active' : ''}`}
                                    >
                                        <Settings size={16} /> Permissions
                                    </button>
                                </div>

                                <div className="tab-content p-6">
                                    {activePlayerTab === 'online' && (
                                        <div className="empty-state">
                                            <Users size={32} style={{ opacity: 0.3 }} />
                                            <p>Aucun joueur connecté.</p>
                                        </div>
                                    )}

                                    {(activePlayerTab === 'whitelist' || activePlayerTab === 'bans' || activePlayerTab === 'permissions') && (
                                        <div className="player-list-manager">
                                            <div className="list-toolbar mb-4 flex gap-2">
                                                <button
                                                    className="btn btn--primary btn--sm"
                                                    onClick={() => {
                                                        const name = prompt('Nom du joueur :');
                                                        if (name) {
                                                            const newItem = activePlayerTab === 'bans'
                                                                ? { name, created: new Date().toISOString(), reason: 'Banned by admin' }
                                                                : activePlayerTab === 'permissions'
                                                                    ? { name, level: 4, bypassesPlayerLimit: false }
                                                                    : { name }; // whitelist
                                                            savePlayerData([...playerData, newItem]);
                                                        }
                                                    }}
                                                >
                                                    <Plus size={16} /> Ajouter un joueur
                                                </button>
                                                <button className="btn btn--secondary btn--sm" onClick={fetchPlayerData}>
                                                    Actualiser
                                                </button>
                                            </div>

                                            {isPlayerLoading ? (
                                                <div className="loading-container py-8 flex justify-center">
                                                    <div className="spinner"></div>
                                                </div>
                                            ) : playerData.length === 0 ? (
                                                <div className="empty-state p-8 border rounded-lg border-dashed">
                                                    <p className="text-muted">La liste est vide.</p>
                                                </div>
                                            ) : (
                                                <div className="data-table-container">
                                                    <table className="data-table w-full text-left">
                                                        <thead>
                                                            <tr className="border-b border-border">
                                                                <th className="p-2">Nom</th>
                                                                {activePlayerTab === 'bans' && <th className="p-2">Raison</th>}
                                                                {activePlayerTab === 'permissions' && <th className="p-2">Niveau</th>}
                                                                <th className="p-2 text-right">Actions</th>
                                                            </tr>
                                                        </thead>
                                                        <tbody>
                                                            {playerData.map((player, idx) => (
                                                                <tr key={idx} className="border-b border-border hover:bg-white/5">
                                                                    <td className="p-2 font-medium">{player.name}</td>
                                                                    {activePlayerTab === 'bans' && <td className="p-2 text-sm text-muted">{player.reason || '-'}</td>}
                                                                    {activePlayerTab === 'permissions' && <td className="p-2 text-sm">{player.level || '1'}</td>}
                                                                    <td className="p-2 text-right">
                                                                        <button
                                                                            className="btn btn--icon btn--danger btn--sm"
                                                                            onClick={() => {
                                                                                if (confirm(`Retirer ${player.name} de la liste ?`)) {
                                                                                    savePlayerData(playerData.filter((_, i) => i !== idx));
                                                                                }
                                                                            }}
                                                                        >
                                                                            <Trash2 size={14} />
                                                                        </button>
                                                                    </td>
                                                                </tr>
                                                            ))}
                                                        </tbody>
                                                    </table>
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </div>
                            </div>
                        </div>
                    )
                }

                {
                    activeTab === 'webhooks' && (
                        <div className="card">
                            <div className="panel-header">
                                <h3 className="panel-header__title"><Webhook size={20} /> Webhooks Discord</h3>
                            </div>
                            <div className="empty-state">
                                <Webhook size={48} className="empty-state-icon" style={{ opacity: 0.2, marginBottom: '1rem' }} />
                                <p className="font-medium">Configurez les notifications Discord</p>
                                <p className="text-sm text-muted mb-4">
                                    Les webhooks sont stockés dans <code className="font-mono bg-dark px-1 rounded">manager.json</code>
                                </p>
                                <button
                                    onClick={() => setActiveTab('files')}
                                    className="btn btn--primary"
                                >
                                    Ouvrir l'éditeur de fichiers
                                </button>
                            </div>
                        </div>
                    )
                }

                {/* Placeholder for unimplemented tabs */}
                {
                    !['console', 'backups', 'files', 'logs', 'config', 'webhooks', 'players'].includes(activeTab) && (
                        <div className="card empty-state">
                            <div style={{ marginBottom: '1.5rem', opacity: 0.3 }}>
                                {tabs.find(t => t.id === activeTab)?.icon}
                            </div>
                            <h3 className="text-xl mb-2 text-muted">Coming Soon</h3>
                            <p>{tabs.find(t => t.id === activeTab)?.label} functionality is under development.</p>
                        </div>
                    )
                }
            </div >
        </div >
    );
}
