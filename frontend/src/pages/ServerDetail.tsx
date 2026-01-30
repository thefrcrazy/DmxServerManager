import {
    BarChart3,
    Clock,
    Cpu,
    HardDrive,
    Play,
    RotateCw,
    Square,
    Terminal, // Keep for tab icon
    Users, // Keep for tab icon
    Settings, // Keep for tab icon
    History, // Keep for tab icon
    FolderOpen, // Keep for tab icon
    FileText, // Keep for tab icon
    Webhook,
    Globe, // Keep for banner stats
} from "lucide-react";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { formatBytes, formatGB } from "../utils/formatters";
import InstallationProgress from "../components/InstallationProgress";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";

// New Components
import ServerConsole from "../components/server/ServerConsole";
import ServerBackups from "../components/server/ServerBackups";
import ServerFiles from "../components/server/ServerFiles";
import ServerLogs from "../components/server/ServerLogs";
import ServerConfig from "../components/server/ServerConfig";
import ServerPlayers from "../components/server/ServerPlayers";
import Tabs from "../components/Tabs";
import WorkInProgress from "../components/WorkInProgress";

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

interface Player {
    name: string;
    is_online: boolean;
    last_seen: string;
    is_op?: boolean;
    is_banned?: boolean;
    is_whitelisted?: boolean;
}

interface Server {
    id: string;
    name: string;
    game_type: string;
    status: string;
    working_dir: string;
    executable_path: string;
    min_memory?: string;
    max_memory?: string;
    java_path?: string;
    extra_args?: string;
    assets_path?: string;
    accept_early_plugins?: boolean;
    auto_start?: boolean;
    disable_sentry?: boolean;
    max_memory_bytes?: number;
    max_heap_bytes?: number;
    memory_usage_bytes?: number;
    cpu_usage?: number;
    disk_usage_bytes?: number;
    bind_address?: string;
    port?: number;
    auth_mode?: "authenticated" | "offline";
    allow_op?: boolean;
    backup_enabled?: boolean;
    backup_dir?: string;
    backup_frequency?: number;
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
    players?: Player[];
    started_at?: string;
}

type TabId =
    | "console"
    | "logs"
    | "schedule"
    | "backups"
    | "files"
    | "config"
    | "players"
    | "metrics"
    | "webhooks";

interface Tab {
    id: TabId;
    label: string;
    icon: React.ReactNode;
}

export default function ServerDetail() {
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { id } = useParams<{ id: string }>();

    const [server, setServer] = useState<Server | null>(null);
    const [searchParams, setSearchParams] = useSearchParams();
    const tabParam = searchParams.get("tab") as TabId | null;
    const [activeTab, setActiveTab] = useState<TabId>(tabParam || "console");

    // Sync activeTab from URL changes (activeTab follows URL)
    useEffect(() => {
        const tab = searchParams.get("tab") as TabId | null;
        if (tab && tabs.some(t => t.id === tab)) {
            setActiveTab(tab);
        }
    }, [searchParams]);

    const handleTabChange = (tabId: TabId) => {
        setActiveTab(tabId);
        setSearchParams({ tab: tabId });
    };
    const [logs, setLogs] = useState<string[]>([]);
    const [isConnected, setIsConnected] = useState(false);
    const [startTime, setStartTime] = useState<Date | null>(null);
    const [uptime, setUptime] = useState("--:--:--");
    const [cpuUsage, setCpuUsage] = useState<number>(0);
    const [ramUsage, setRamUsage] = useState<number>(0);
    const [diskUsage, setDiskUsage] = useState<number | null>(null);
    const wsRef = useRef<WebSocket | null>(null);

    // Backups tab state
    const [backups, setBackups] = useState<Backup[]>([]);
    const [backupsLoading, setBackupsLoading] = useState(false);
    const [creatingBackup, setCreatingBackup] = useState(false);

    // Files tab state
    const [files, setFiles] = useState<FileEntry[]>([]);
    const [currentPath, setCurrentPath] = useState("");
    const [filesLoading, setFilesLoading] = useState(false);
    const [selectedFile, setSelectedFile] = useState<string | null>(null);
    const [fileContent, setFileContent] = useState("");
    const [fileSaving, setFileSaving] = useState(false);

    // Logs tab state
    const [logFiles, setLogFiles] = useState<FileEntry[]>([]);
    const [selectedLogFile, setSelectedLogFile] = useState<string | null>(null);
    const [logContent, setLogContent] = useState("");

    // Installation state
    const [isInstalling, setIsInstalling] = useState(false);
    const [isAuthRequired, setIsAuthRequired] = useState(false);

    // Config tab state
    const [configFormData, setConfigFormData] = useState<Partial<Server>>({});
    const [configSaving, setConfigSaving] = useState(false);
    const [configError, setConfigError] = useState("");
    const [javaVersions, setJavaVersions] = useState<
        { path: string; version: string }[]
    >([]);

    // Players tab state
    const [activePlayerTab, setActivePlayerTab] = useState<
        "online" | "whitelist" | "bans" | "ops"
    >("online");
    const [playerData, setPlayerData] = useState<any[]>([]); // For file-based lists


    const tabs: Tab[] = [
        { id: "console", label: t("server_detail.tabs.terminal"), icon: <Terminal size={18} /> },
        { id: "logs", label: t("server_detail.tabs.logs"), icon: <FileText size={18} /> },
        { id: "backups", label: t("server_detail.tabs.backups"), icon: <History size={18} /> },
        { id: "files", label: t("server_detail.tabs.files"), icon: <FolderOpen size={18} /> },
        { id: "config", label: t("server_detail.tabs.config"), icon: <Settings size={18} /> },
        { id: "players", label: t("server_detail.tabs.players"), icon: <Users size={18} /> },
        { id: "metrics", label: t("server_detail.tabs.metrics"), icon: <BarChart3 size={18} /> },
        { id: "webhooks", label: t("server_detail.tabs.webhooks"), icon: <Webhook size={18} /> },
    ];

    const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
    const retryCountRef = useRef(0);
    const shouldReconnectRef = useRef(true);
    const serverStatusRef = useRef(server?.status);

    useEffect(() => {
        serverStatusRef.current = server?.status;

        setIsInstalling(server?.status === "installing");
        setIsAuthRequired(server?.status === "auth_required");

        if (
            (server?.status === "running" ||
                server?.status === "installing" ||
                server?.status === "auth_required") &&
            !wsRef.current
        ) {
            shouldReconnectRef.current = true;
            connectWebSocket();
        }

        if (server?.status === "stopped" || server?.status === "offline") {
            setIsAuthRequired(false);
            setIsInstalling(false);
        }
    }, [server?.status]);

    useEffect(() => {
        setLogs([]);
        fetchServer();
        fetchConsoleLog();

        return () => {
            shouldReconnectRef.current = false;
            if (wsRef.current) {
                wsRef.current.close();
                wsRef.current = null;
            }
            if (reconnectTimeoutRef.current) {
                clearTimeout(reconnectTimeoutRef.current);
            }
        };
    }, [id]);

    const connectWebSocket = () => {
        if (wsRef.current) {
            if (wsRef.current.readyState === WebSocket.OPEN || wsRef.current.readyState === WebSocket.CONNECTING) return;
            wsRef.current.onclose = null;
            wsRef.current.close();
        }

        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        // Fix: Backend WS endpoint is under /api/v1
        const ws = new WebSocket(`${protocol}//${window.location.host}/api/v1/ws/console/${id}`);

        ws.onopen = () => {
            setIsConnected(true);
            retryCountRef.current = 0;
            // Fix: Re-fetch full logs history on connection/reconnection to ensure we didn't miss anything
            fetchConsoleLog();
        };

        ws.onmessage = (event) => {
            const message = event.data;

            if (message.startsWith("[STATUS]:")) {
                const status = message.replace("[STATUS]:", "").trim();
                setServer((prev) => (prev ? { ...prev, status } : null));
                if (status === "running") setStartTime(new Date());
                else setStartTime(null);
                fetchServer();
                return;
            }

            if (message.trim().startsWith("[METRICS]:")) {
                try {
                    const metrics = JSON.parse(message.trim().substring(10));
                    setCpuUsage(metrics.cpu || 0);
                    setRamUsage(metrics.memory || 0);
                    if (metrics.disk_bytes !== undefined) setDiskUsage(metrics.disk_bytes);
                } catch (e) {
                    console.error("Failed to parse metrics", e);
                }
                return;
            }

            if (message.includes("Initialization of installation") || message.includes("Initialization de l'installation")) {
                setIsInstalling(true);
                setIsAuthRequired(false);
            }
            if (message.includes("IMPORTANT") && (message.includes("authentifier") || message.includes("authenticate"))) {
                if (server?.status === "running" || server?.status === "starting") {
                    setIsAuthRequired(true);
                }
            }
            if (message.includes("Authentication successful!") || message.includes("Success!")) {
                setIsAuthRequired(false);
            }
            if (message.includes("Installation terminée") || message.includes("Installation finished")) {
                setIsInstalling(false);
                fetchServer();
            }

            setLogs((prev) => [...prev, message]);
        };

        ws.onclose = () => {
            setIsConnected(false);
            wsRef.current = null;
            const shouldRetry = shouldReconnectRef.current && (serverStatusRef.current === "running" || serverStatusRef.current === "installing" || serverStatusRef.current === "auth_required");
            if (shouldRetry) {
                const retryDelay = Math.min(1000 * Math.pow(1.5, retryCountRef.current), 10000);
                reconnectTimeoutRef.current = setTimeout(() => {
                    retryCountRef.current++;
                    connectWebSocket();
                }, retryDelay);
            }
        };

        ws.onerror = (err) => console.error("WebSocket error:", err);
        wsRef.current = ws;
    };

    const fetchConsoleLog = async () => {
        if (!id) return;
        try {
            let installRes = await fetch(`/api/v1/servers/${id}/files/read?path=logs/install.log`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });

            if (installRes.ok) {
                const data = await installRes.json();
                if (data.content) {
                    const lines = data.content.split("\n");
                    const hasStart = lines.some((l: string) => l.includes("Initialization de l'installation") || l.includes("Starting Hytale Server Installation"));
                    const hasEnd = lines.some((l: string) => l.includes("Installation terminée") || l.includes("Installation finished"));
                    if (hasStart && !hasEnd && server?.status !== "running") setIsInstalling(true);
                }
            }

            let res = await fetch(`/api/v1/servers/${id}/files/read?path=logs/console.log`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });

            if (res.ok) {
                const data = await res.json();
                if (data.content && data.content.length > 0) setLogs(data.content.split("\n"));
            } else if (installRes.ok) {
                const data = await installRes.json();
                if (data.content) setLogs(data.content.split("\n"));
            }
        } catch (e) { }
    };

    const sendCommand = (cmd: string) => {
        if (cmd.trim() && wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(cmd);
            setLogs((prev) => [...prev, `> ${cmd}`]);
        }
    };

    // Uptime
    useEffect(() => {
        if (server?.status === "running" && startTime) {
            const interval = setInterval(() => {
                const diff = Date.now() - startTime.getTime();
                const hours = Math.floor(diff / 3600000);
                const minutes = Math.floor((diff % 3600000) / 60000);
                const seconds = Math.floor((diff % 60000) / 1000);
                setUptime(`${hours.toString().padStart(2, "0")}:${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`);
            }, 1000);
            return () => clearInterval(interval);
        } else {
            setUptime("--:--:--");
        }
    }, [server?.status, startTime]);

    // Data Fetching Handlers
    const fetchServer = useCallback(async () => {
        const response = await fetch(`/api/v1/servers/${id}`, {
            headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
        });
        const data = await response.json();
        setServer(data);
        if (data.disk_usage_bytes !== undefined) setDiskUsage(data.disk_usage_bytes);
        if (data.status === "running" && data.started_at) setStartTime(new Date(data.started_at));
        else if (data.status !== "running") setStartTime(null);
    }, [id]);

    const handleAction = useCallback(async (action: "start" | "stop" | "restart" | "kill") => {
        if (action === "start" && server?.status === "running") return;
        try {
            const res = await fetch(`/api/v1/servers/${id}/${action}`, {
                method: "POST",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (!res.ok) {
                const data = await res.json();
                if (res.status === 400 && action === "start" && data.error === "Server already running") {
                    fetchServer();
                    return;
                }
                alert(t("server_detail.messages.action_error"));
                return;
            }
            if (action === "start") { setLogs([]); setIsAuthRequired(false); }
            else if (action === "stop" || action === "kill") { setStartTime(null); setIsAuthRequired(false); setLogs([]); }
            fetchServer();
            setTimeout(fetchServer, 1000);
            setTimeout(fetchServer, 3000);
        } catch (e) {
            console.error(e);
            alert(t("server_detail.messages.connection_error"));
        }
    }, [id, server, t, fetchServer]);

    const fetchBackups = useCallback(async () => {
        if (!id) return;
        setBackupsLoading(true);
        try {
            const response = await fetch(`/api/v1/backups?server_id=${id}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            const data = await response.json();
            setBackups(data);
        } catch (error) { console.error(error); } finally { setBackupsLoading(false); }
    }, [id]);

    const createBackup = async () => {
        if (!id) return;
        setCreatingBackup(true);
        try {
            await fetch("/api/v1/backups", {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ server_id: id }),
            });
            fetchBackups();
        } catch (error) { console.error(error); } finally { setCreatingBackup(false); }
    };

    const deleteBackup = async (backupId: string) => {
        if (!confirm(t("server_detail.delete_backup_confirm"))) return;
        try {
            await fetch(`/api/v1/backups/${backupId}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            fetchBackups();
        } catch (error) { console.error(error); }
    };

    const restoreBackup = async (backupId: string) => {
        if (!confirm(t("server_detail.restore_backup_confirm"))) return;
        try {
            await fetch(`/api/v1/backups/${backupId}/restore`, {
                method: "POST",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            alert(t("server_detail.messages.backup_restored"));
        } catch (error) { console.error(error); }
    };

    const fetchFiles = useCallback(async (path = "") => {
        if (!id) return;
        setFilesLoading(true);
        try {
            const response = await fetch(`/api/v1/servers/${id}/files?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            const data = await response.json();
            setFiles(data.entries || []);
            setCurrentPath(data.current_path || "");
        } catch (error) { console.error(error); } finally { setFilesLoading(false); }
    }, [id]);

    const readFile = async (path: string) => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            const data = await response.json();
            setFileContent(data.content || "");
            setSelectedFile(path);
        } catch (error) { console.error(error); }
    };

    const saveFile = async (content: string) => {
        if (!id || !selectedFile) return;
        setFileSaving(true);
        try {
            await fetch(`/api/v1/servers/${id}/files/write`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ path: selectedFile, content }),
            });
            alert(t("server_detail.messages.file_saved"));
        } catch (error) { console.error(error); } finally { setFileSaving(false); }
    };

    const fetchLogFiles = async () => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files?path=logs`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            const data = await response.json();
            let logs = (data.entries || []).filter((f: FileEntry) => !f.is_dir);
            logs.sort((a: FileEntry, b: FileEntry) => b.name.localeCompare(a.name));

            const bottomLogs = ["console.log", "install.log"];
            const specialLogs: FileEntry[] = [];
            logs = logs.filter((l: FileEntry) => {
                if (l.name.endsWith(".lck")) return false;
                if (bottomLogs.includes(l.name)) { specialLogs.push(l); return false; }
                return true;
            });
            if (!specialLogs.some((l) => l.name === "console.log")) {
                specialLogs.push({ name: "console.log", path: "logs/console.log", is_dir: false });
            }
            logs.push(...specialLogs);
            setLogFiles(logs);
            if (logs.length > 0 && !selectedLogFile) readLogFile(logs[0].path);
        } catch (error) { console.error(error); }
    };

    const readLogFile = async (path: string) => {
        if (!id) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${encodeURIComponent(path)}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            const data = await response.json();
            setLogContent(data.content || "");
            setSelectedLogFile(path);
        } catch (error) { console.error(error); }
    };

    // Config Logic
    useEffect(() => {
        if (activeTab === "config") {
            const fetchJavaVersions = async () => {
                try {
                    const response = await fetch("/api/v1/system/java-versions", {
                        headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
                    });
                    if (response.ok) setJavaVersions(await response.json());
                } catch (error) { }
            };
            fetchJavaVersions();
            if (server && configFormData.id !== server.id) {
                const formData = { ...server };
                if (server.config) {
                    // Map config... (reusing logic from snippet)
                    Object.assign(formData, server.config); // Simple merge for now as keys match roughly except JSON specific
                    // Specific mappings if keys differ
                    if (server.config.MaxPlayers) formData.max_players = server.config.MaxPlayers;
                    if (server.config.MaxViewRadius) formData.view_distance = server.config.MaxViewRadius;
                    if (server.config.Seed) formData.seed = server.config.Seed;
                    if (server.config.ServerName) formData.name = server.config.ServerName;
                }
                setConfigFormData(formData);
            }
        }
    }, [activeTab, server, configFormData.id]);

    const updateConfigValue = <K extends keyof Server>(key: K, value: Server[K]) => {
        setConfigFormData((prev) => ({ ...prev, [key]: value }));
    };

    const toggleJvmArg = (arg: string) => {
        let currentArgs = configFormData.extra_args || "";
        let parts = currentArgs.trim().split(/\s+/).filter((a) => a.length > 0);
        if (parts.includes(arg)) parts = parts.filter((a) => a !== arg);
        else parts.push(arg);
        updateConfigValue("extra_args", parts.join(" "));
    };

    const handleSaveConfig = async (e: React.FormEvent) => {
        e.preventDefault();
        configFormData.id = id;
        setConfigSaving(true);
        setConfigError("");
        try {
            const payload = { ...configFormData };
            if (!payload.config) payload.config = server?.config || {};

            // Sync back to config object
            if (payload.max_players) payload.config.MaxPlayers = Number(payload.max_players);
            if (payload.view_distance) payload.config.MaxViewRadius = Number(payload.view_distance);
            if (payload.seed) payload.config.Seed = payload.seed;
            if (payload.name) payload.config.ServerName = payload.name;
            if (payload.port) payload.config.port = Number(payload.port);
            if (payload.bind_address) payload.config.bind_address = payload.bind_address;
            if (payload.auth_mode) payload.config.auth_mode = payload.auth_mode;
            if (payload.allow_op !== undefined) payload.config.allow_op = payload.allow_op;
            if (payload.disable_sentry !== undefined) payload.config.disable_sentry = payload.disable_sentry;
            if (payload.accept_early_plugins !== undefined) payload.config.accept_early_plugins = payload.accept_early_plugins;

            const response = await fetch(`/api/v1/servers/${id}`, {
                method: "PUT",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify(payload),
            });
            if (response.ok) { fetchServer(); alert(t("server_detail.messages.config_saved")); }
            else { const data = await response.json(); setConfigError(data.error || t("server_detail.messages.save_error")); }
        } catch (err) { setConfigError(t("server_detail.messages.connection_error")); }
        finally { setConfigSaving(false); }
    };

    // Players Logic
    const fetchPlayerData = useCallback(async () => {
        if (!id || activePlayerTab === "online") return;
        let filename = "";
        if (activePlayerTab === "whitelist") filename = "server/whitelist.json";
        else if (activePlayerTab === "bans") filename = "server/bans.json";
        else if (activePlayerTab === "ops") filename = "server/ops.json";

        try {
            const response = await fetch(`/api/v1/servers/${id}/files/read?path=${filename}`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                const data = await response.json();
                try {
                    const parsed = JSON.parse(data.content);
                    setPlayerData(Array.isArray(parsed) ? parsed : []);
                } catch (e) { setPlayerData([]); }
            } else { setPlayerData([]); }
        } catch (error) { setPlayerData([]); }
    }, [id, activePlayerTab]);

    // Simple handler mocks for players since logic was complex and file-based
    const onPlayerAction = (action: string, name: string) => {
        // Implement kick/ban/op via API or Console Command
        if (action === "op") sendCommand(`op ${name}`);
        else if (action === "kick") sendCommand(`kick ${name}`);
        else if (action === "ban") sendCommand(`ban ${name}`);
    };

    // Effect triggers
    useEffect(() => {
        if (activeTab === "backups") fetchBackups();
        else if (activeTab === "files") { fetchFiles(); setSelectedFile(null); setFileContent(""); }
        else if (activeTab === "logs") fetchLogFiles();
        else if (activeTab === "players") { /* fetch players list */ }
    }, [activeTab, fetchBackups, fetchFiles]);

    // Page Title
    useEffect(() => {
        if (server) {
            setPageTitle(server.name, `${server.game_type} Server`, { to: "/servers" },
                <div className="header-actions-group">
                    <div className={`status-badge-large ${server.status === "running" ? "status-badge-large--online" : server.status === "missing" ? "status-badge-large--error" : "status-badge-large--offline"}`}>
                        <span className="status-dot"></span>
                        {server.status}
                    </div>
                    <div className="header-controls">
                        <button className="btn btn--sm btn--primary" onClick={() => handleAction("start")} disabled={server.status === "running"}><Play size={16} /> Start</button>
                        <button className="btn btn--sm btn--secondary" onClick={() => handleAction("restart")} disabled={server.status !== "running"}><RotateCw size={16} /></button>
                        <button className="btn btn--sm btn--danger" onClick={() => handleAction("stop")} disabled={server.status !== "running"}><Square size={16} /></button>
                    </div>
                </div>
            );
        }
    }, [server, setPageTitle, handleAction, t]);

    if (!server) return <div className="loading-screen"><div className="spinner"></div></div>;

    return (
        <div className="server-detail-page">
            <div className="server-header-stats">
                {/* ... Stats Pills (Keep as is, they were fine) ... */}
                <div className="stat-pill"><div className="stat-pill__icon"><Clock size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">UPTIME</div><div className="stat-pill__value">{uptime}</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><Users size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">PLAYERS</div><div className="stat-pill__value">{server.players?.filter(p => p.is_online).length || 0} / {server.max_players || 100}</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><Globe size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">ADDRESS</div><div className="stat-pill__value">{server.bind_address}:{server.port}</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><Cpu size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">CPU</div><div className="stat-pill__value">{Math.round(cpuUsage)}%</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><HardDrive size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">RAM</div><div className="stat-pill__value">{formatGB(ramUsage)}</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><HardDrive size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">DISK</div><div className="stat-pill__value">{diskUsage !== null ? formatBytes(diskUsage) : "0 B"}</div></div></div>
            </div>

            <Tabs tabs={tabs} activeTab={activeTab} onTabChange={handleTabChange} />

            {isInstalling && <InstallationProgress logs={logs} isInstalling={isInstalling} isAuthRequired={isAuthRequired} onClose={() => setIsInstalling(false)} onSendAuth={() => sendCommand("auth")} />}

            <div className="tab-content">
                {activeTab === "console" && (
                    <ServerConsole
                        logs={logs}
                        isConnected={isConnected}
                        isRunning={server.status === "running" || server.status === "starting"}
                        onSendCommand={sendCommand}
                    />
                )}

                {activeTab === "backups" && (
                    <ServerBackups
                        backups={backups}
                        isLoading={backupsLoading}
                        isCreating={creatingBackup}
                        onCreateBackup={createBackup}
                        onRestoreBackup={restoreBackup}
                        onDeleteBackup={deleteBackup}
                    />
                )}

                {activeTab === "files" && (
                    <ServerFiles
                        files={files}
                        currentPath={currentPath}
                        isLoading={filesLoading}
                        selectedFile={selectedFile}
                        fileContent={fileContent}
                        isSaving={fileSaving}
                        onNavigate={fetchFiles}
                        onReadFile={readFile}
                        onSaveFile={saveFile}
                        onCloseEditor={() => setSelectedFile(null)}
                        onRefresh={() => fetchFiles(currentPath)}
                    />
                )}

                {activeTab === "logs" && (
                    <ServerLogs
                        logFiles={logFiles}
                        selectedLogFile={selectedLogFile}
                        logContent={logContent}
                        onSelectLogFile={readLogFile}
                        onRefresh={fetchLogFiles}
                    />
                )}

                {activeTab === "config" && (
                    <ServerConfig
                        configFormData={configFormData}
                        configSaving={configSaving}
                        configError={configError}
                        javaVersions={javaVersions}
                        updateConfigValue={updateConfigValue}
                        toggleJvmArg={toggleJvmArg}
                        handleSaveConfig={handleSaveConfig}
                        t={t}
                    />
                )}

                {activeTab === "players" && (
                    <ServerPlayers
                        players={server.players || []} // Online players
                        playerList={playerData} // Whitelist/Ban lists
                        activeTab={activePlayerTab}
                        onTabChange={setActivePlayerTab}
                        isLoading={false}
                        onAction={onPlayerAction}
                        onAddPlayer={(name) => {
                            if (activePlayerTab === "whitelist") sendCommand(`whitelist add ${name}`);
                            else if (activePlayerTab === "ops") sendCommand(`op ${name}`);
                        }}
                        onRemovePlayer={(name) => {
                            if (activePlayerTab === "whitelist") sendCommand(`whitelist remove ${name}`);
                            else if (activePlayerTab === "ops") sendCommand(`deop ${name}`);
                            else if (activePlayerTab === "bans") sendCommand(`pardon ${name}`);
                        }}
                        onRefresh={() => {
                            fetchServer();
                            fetchPlayerData();
                        }}
                    />
                )}

                {(activeTab === "metrics" || activeTab === "webhooks") && (
                    <WorkInProgress />
                )}
            </div>
        </div>
    );
}
