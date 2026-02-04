import {
    BarChart3,
    Clock,
    Cpu,
    HardDrive,
    Play,
    RotateCw,
    Square,
    Terminal,
    Users,
    Settings,
    History,
    FolderOpen,
    FileText,
    Webhook,
    Globe,
    Package,
} from "lucide-react";
import React, { useCallback, useEffect, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { formatBytes, formatGB } from "../utils/formatters";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";
import { useServerWebSocket } from "../hooks";
import { useToast } from "../contexts/ToastContext";
import { useDialog } from "../contexts/DialogContext";

// New Components
import {
    ServerConsole,
    ServerBackups,
    ServerFiles,
    ServerLogs,
    ServerConfig,
    ServerPlayers,
    ServerMetrics,
    AddPlayerModal
} from "@/components/features/server";
import { Tabs } from "@/components/ui";
import { WorkInProgress, InstallationProgress } from "@/components/shared";

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

// Don't define local Player interface, import or redefine compatible one if needed.
// Actually, to avoid conflicts, let's just make the local one compatible or remove it if I import.
// For now, I will align the local interface with ServerPlayers requirement.

interface Player {
    name: string;
    uuid?: string;
    is_online: boolean;
    last_seen?: string;
    is_op?: boolean;
    is_banned?: boolean;
    is_whitelisted?: boolean;
    reason?: string;
    bannedBy?: string;
    expires?: string;
    created?: string;
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
    | "mods"
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
    const { success, error: showError } = useToast();
    const { confirm } = useDialog();
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

    // Data Fetching Handlers - defined before the hook to avoid circular dependency
    const fetchServer = useCallback(async () => {
        const response = await fetch(`/api/v1/servers/${id}`, {
            headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
        });
        const data = await response.json();
        setServer(data);
    }, [id]);

    // Use WebSocket hook for console, metrics and installation state
    const {
        logs,
        setLogs,
        isConnected,
        cpuUsage,
        ramUsage,
        memoryLimit,
        diskUsage,
        startTime,
        setStartTime,
        isInstalling,
        setIsInstalling,
        isAuthRequired,
        setIsAuthRequired,
        sendCommand,
        currentPlayers,
        currentPlayersList,
    } = useServerWebSocket({
        serverId: id,
        serverStatus: server?.status,
        onServerUpdate: fetchServer,
        onStatusChange: useCallback((status: string) => setServer((prev) => (prev ? { ...prev, status } : null)), []),
    });

    // Initial server fetch on mount
    useEffect(() => {
        fetchServer();
    }, [fetchServer]);

    const [uptime, setUptime] = useState("--:--:--");

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

    // Config tab state
    const [configFormData, setConfigFormData] = useState<Partial<Server>>({});
    const [initialConfigFormData, setInitialConfigFormData] = useState<Partial<Server>>({});
    const [configSaving, setConfigSaving] = useState(false);
    const [configError, setConfigError] = useState("");
    const [javaVersions, setJavaVersions] = useState<
        { path: string; version: string }[]
    >([]);

    // Players tab state
    const [activePlayerTab, setActivePlayerTab] = useState<"online" | "whitelist" | "bans" | "ops" | "database">("online");
    const [playerData, setPlayerData] = useState<Player[]>([]);
    const [showAddPlayerModal, setShowAddPlayerModal] = useState(false);


    const tabs: Tab[] = [
        { id: "console", label: t("server_detail.tabs.terminal"), icon: <Terminal size={18} /> },
        { id: "logs", label: t("server_detail.tabs.logs"), icon: <FileText size={18} /> },
        { id: "backups", label: t("server_detail.tabs.backups"), icon: <History size={18} /> },
        { id: "files", label: t("server_detail.tabs.files"), icon: <FolderOpen size={18} /> },
        { id: "config", label: t("server_detail.tabs.config"), icon: <Settings size={18} /> },
        { id: "mods", label: t("server_detail.tabs.mods"), icon: <Package size={18} /> },
        { id: "players", label: t("server_detail.tabs.players"), icon: <Users size={18} /> },
        { id: "metrics", label: t("server_detail.tabs.metrics"), icon: <BarChart3 size={18} /> },
        { id: "webhooks", label: t("server_detail.tabs.webhooks"), icon: <Webhook size={18} /> },
    ];

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

    // Sync startTime from server data when fetched
    useEffect(() => {
        if (server?.status === "running" && server?.started_at) {
            setStartTime(new Date(server.started_at));
        } else if (server?.status !== "running") {
            setStartTime(null);
        }
    }, [server?.status, server?.started_at, setStartTime]);

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
                showError(t("server_detail.messages.action_error"));
                return;
            }
            if (action === "start") { setLogs([]); setIsAuthRequired(false); }
            else if (action === "stop" || action === "kill") { setStartTime(null); setIsAuthRequired(false); setLogs([]); }
            fetchServer();
            setTimeout(fetchServer, 1000);
            setTimeout(fetchServer, 3000);
        } catch (e) {
            console.error(e);
            showError(t("server_detail.messages.connection_error"));
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
        if (!await confirm(t("server_detail.delete_backup_confirm"), { isDestructive: true })) return;
        try {
            await fetch(`/api/v1/backups/${backupId}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            fetchBackups();
        } catch (error) { console.error(error); }
    };

    const restoreBackup = async (backupId: string) => {
        if (!await confirm(t("server_detail.restore_backup_confirm"), { isDestructive: true })) return;
        try {
            await fetch(`/api/v1/backups/${backupId}/restore`, {
                method: "POST",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            success(t("server_detail.messages.backup_restored"));
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
            success(t("server_detail.messages.file_saved"));
        } catch (error) { console.error(error); } finally { setFileSaving(false); }
    };

    const createFolder = async (name: string) => {
        if (!id || !name.trim()) return;
        try {
            const folderPath = currentPath ? `${currentPath}/${name}` : name;
            const response = await fetch(`/api/v1/servers/${id}/files/mkdir`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ path: folderPath }),
            });
            if (response.ok) {
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const createFile = async (name: string) => {
        if (!id || !name.trim()) return;
        try {
            const filePath = currentPath ? `${currentPath}/${name}` : name;
            const response = await fetch(`/api/v1/servers/${id}/files/create`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ path: filePath }),
            });
            if (response.ok) {
                fetchFiles(currentPath);
                readFile(filePath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const uploadFiles = async (files: FileList) => {
        if (!id || files.length === 0) return;
        try {
            const formData = new FormData();
            formData.append("path", currentPath);
            for (let i = 0; i < files.length; i++) {
                formData.append("files", files[i]);
            }
            const response = await fetch(`/api/v1/servers/${id}/files/upload`, {
                method: "POST",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: formData,
            });
            if (response.ok) {
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const deleteFile = async (path: string) => {
        if (!id || !path) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/delete`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ path }),
            });
            if (response.ok) {
                if (selectedFile === path) {
                    setSelectedFile(null);
                    setFileContent("");
                }
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const renameFile = async (path: string, newName: string) => {
        if (!id || !path || !newName.trim()) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/rename`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ path, new_name: newName }),
            });
            if (response.ok) {
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const copyFile = async (source: string, destination: string) => {
        if (!id || !source || !destination) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/copy`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ source, destination }),
            });
            if (response.ok) {
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
    };

    const moveFile = async (source: string, destination: string) => {
        if (!id || !source || !destination) return;
        try {
            const response = await fetch(`/api/v1/servers/${id}/files/move`, {
                method: "POST",
                headers: { "Content-Type": "application/json", Authorization: `Bearer ${localStorage.getItem("token")}` },
                body: JSON.stringify({ source, destination }),
            });
            if (response.ok) {
                fetchFiles(currentPath);
            } else {
                const data = await response.json();
                showError(data.error || t("server_detail.messages.action_error"));
            }
        } catch (error) { console.error(error); }
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
            if (server) {
                // If we haven't initialized yet, OR if the server ID changed (e.g. navigation)
                const isNewServer = configFormData.id !== server.id;

                // Only reset if it's a new server or we explicitly want to re-sync (e.g. after external update)
                // Note: We don't want to overwrite user inputs if they are typing, but here we only sync on mount/tab-change/id-change or explicit flush.
                if (isNewServer) {
                    // Create a temporary object with all possible data merged
                    const fullData: any = { ...server };
                    if (server.config) {
                        Object.assign(fullData, server.config);
                        if (server.config.MaxPlayers) fullData.max_players = server.config.MaxPlayers;
                        if (server.config.MaxViewRadius) fullData.view_distance = server.config.MaxViewRadius;
                        if (server.config.Seed) fullData.seed = server.config.Seed;
                        if (server.config.ServerName) fullData.name = server.config.ServerName;
                    }

                    // Whitelist only the fields we edit in the form to avoid noise from other server props
                    const configKeys = [
                        "id",
                        // Mandatory fields for backend validation (CreateServerRequest)
                        "name", "game_type", "executable_path", "working_dir",
                        "auth_mode",
                        "min_memory", "max_memory", "java_path", "extra_args",
                        "bind_address", "port",
                        "allow_op", "disable_sentry", "accept_early_plugins",
                        "world_gen_type", "seed", "view_distance", "max_players",
                        "is_pvp_enabled", "is_fall_damage_enabled", "is_spawning_npc",
                        "is_game_time_paused", "is_saving_players", "is_saving_chunks"
                    ];

                    const cleanData: Partial<Server> = {};
                    configKeys.forEach(key => {
                        if (fullData[key] !== undefined) {
                            // @ts-ignore
                            cleanData[key] = fullData[key];
                        }
                    });

                    setConfigFormData(cleanData);
                    setInitialConfigFormData(JSON.parse(JSON.stringify(cleanData)));
                }
            }
        }
    }, [activeTab, server, configFormData.id]);

    const updateConfigValue = <K extends keyof Server>(key: K, value: Server[K]) => {
        setConfigFormData((prev) => ({ ...prev, [key]: value }));
    };

    const toggleJvmArg = (arg: string) => {
        const currentArgs = configFormData.extra_args || "";
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
            if (response.ok) {
                fetchServer();
                // Fix: Update initial state to match the saved state so "has changes" becomes false
                setInitialConfigFormData(JSON.parse(JSON.stringify(configFormData)));
                success(t("server_detail.messages.config_saved"));
            }
            else { const data = await response.json(); setConfigError(data.error || t("server_detail.messages.save_error")); }
        } catch (err) { setConfigError(t("server_detail.messages.connection_error")); }
        finally { setConfigSaving(false); }
    };



    const handleDeleteServer = async () => {
        if (!await confirm(t("server_detail.messages.delete_confirm"), { isDestructive: true })) return;
        try {
            const res = await fetch(`/api/v1/servers/${id}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (res.ok) {
                success(t("server_detail.messages.delete_success"));
                setPageTitle("Dashboard", "", { to: "/" }); // Reset title
                window.location.href = "/";
            } else {
                showError(t("server_detail.messages.delete_error"));
            }
        } catch (e) { console.error(e); }
    };

    const handleReinstallServer = async () => {
        if (!await confirm(t("server_detail.messages.reinstall_confirm"), { isDestructive: true })) return;
        try {
            const res = await fetch(`/api/v1/servers/${id}/reinstall`, {
                method: "POST",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (res.ok) {
                setIsInstalling(true);
                setLogs([]);
                // Re-fetch server to get updated status if needed, though socket will do it
            } else {
                showError(t("server_detail.messages.action_error"));
            }
        } catch (e) { console.error(e); }
    };

    // Players Logic
    const fetchPlayerData = async () => {
        if (!server) return;

        try {
            let list: Player[] = [];

            // Always fetch online players from server status
            const online = server.players || [];

            if (activePlayerTab === "online") {
                list = online;
            } else if (activePlayerTab === "database") {
                if (server.players) {
                    list = server.players.map(p => ({
                        ...p,
                        is_online: false,
                    }));
                }
            } else if (activePlayerTab === "whitelist") {
                const res = await fetch(`/api/v1/servers/${id}/whitelist`);
                if (res.ok) {
                    const data = await res.json();
                    list = data.map((entry: any) => ({
                        name: entry.name || "Inconnu",
                        uuid: entry.uuid,
                        is_online: false,
                        last_seen: "",
                        is_whitelisted: true
                    }));
                }
            } else if (activePlayerTab === "bans") {
                const res = await fetch(`/api/v1/servers/${id}/bans`);
                if (res.ok) {
                    const data = await res.json();
                    list = data.map((b: any) => ({
                        name: b.username || "Inconnu (" + (b.target || "?") + ")",
                        uuid: b.target,
                        reason: b.reason,
                        bannedBy: b.by,
                        expires: b.type === "infinite" ? undefined : b.expires,
                        created: b.timestamp ? new Date(b.timestamp).toISOString() : undefined,
                        is_online: false,
                        last_seen: "",
                        is_banned: true
                    }));
                }
            } else if (activePlayerTab === "ops") {
                const res = await fetch(`/api/v1/servers/${id}/ops`);
                if (res.ok) {
                    const data = await res.json();
                    list = data.map((op: any) => ({
                        name: "UUID: " + op.uuid.substring(0, 8),
                        uuid: op.uuid,
                        reason: (op.groups || []).join(", "),
                        is_op: true,
                        is_online: false,
                        last_seen: ""
                    }));
                }
            }
            setPlayerData(list);
        } catch (e) {
            console.error("Failed to fetch player data", e);
        }
    };

    const handlePlayerAction = async (action: string, player: Player) => {
        if (!player.name) return;

        // Online actions via commands
        if (server?.status === "running") {
            if (action === "op") await sendCommand(`op ${player.name}`);
            else if (action === "deop") await sendCommand(`deop ${player.name}`);
            else if (action === "kick") await sendCommand(`kick ${player.name}`);
            else if (action === "ban") await sendCommand(`ban ${player.name}`);
            else if (action === "unban") {
                await sendCommand(`pardon ${player.name}`);
            }

            setTimeout(() => {
                fetchServer();
                fetchPlayerData();
            }, 1000);
        } else {
            showError(t("server_detail.messages.server_offline_action"));
        }
    };

    const handleAddPlayer = async () => {
        setShowAddPlayerModal(true);
    };

    const handleConfirmAddPlayer = async (name: string) => {
        setShowAddPlayerModal(false);
        if (!name) return;

        if (server?.status === "running") {
            if (activePlayerTab === "whitelist") sendCommand(`whitelist add ${name}`);
            else if (activePlayerTab === "ops") sendCommand(`op ${name}`);
            else if (activePlayerTab === "bans") sendCommand(`ban ${name}`);
            setTimeout(fetchPlayerData, 1000);
        } else {
            // Offline API usage
            try {
                if (activePlayerTab === "whitelist") {
                    await fetch(`/api/v1/servers/${id}/whitelist`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ name, uuid: name }) // Fallback uuid=name if unknown
                    });
                } else if (activePlayerTab === "ops") {
                    await fetch(`/api/v1/servers/${id}/ops`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ uuid: name }) // Prompt is name/uuid entry
                    });
                } else if (activePlayerTab === "bans") {
                    await fetch(`/api/v1/servers/${id}/bans`, {
                        method: "POST",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ target: name, reason: "Banned via Panel" })
                    });
                }
                success(t("server_detail.messages.save_success"));
                fetchPlayerData();
            } catch (e) {
                showError("Erreur lors de l'ajout via API");
            }
        }
    };

    const handleRemovePlayer = async (player: Player) => {
        if (!await confirm(t("server_detail.players.remove_confirm"), { isDestructive: true })) return;

        if (server?.status === "running") {
            if (activePlayerTab === "whitelist") sendCommand(`whitelist remove ${player.name}`);
            else if (activePlayerTab === "ops") sendCommand(`deop ${player.name}`);
            else if (activePlayerTab === "bans") sendCommand(`pardon ${player.name}`);
            setTimeout(fetchPlayerData, 1000);
        } else {
            try {
                if (activePlayerTab === "whitelist") {
                    await fetch(`/api/v1/servers/${id}/whitelist`, {
                        method: "DELETE",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ name: player.name, uuid: player.uuid })
                    });
                } else if (activePlayerTab === "ops") {
                    await fetch(`/api/v1/servers/${id}/ops`, {
                        method: "DELETE",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ uuid: player.uuid })
                    });
                } else if (activePlayerTab === "bans") {
                    await fetch(`/api/v1/servers/${id}/bans`, {
                        method: "DELETE",
                        headers: { "Content-Type": "application/json" },
                        body: JSON.stringify({ target: player.uuid })
                    });
                }
                success(t("server_detail.messages.save_success"));
                fetchPlayerData();
            } catch (e) {
                showError("Erreur lors de la suppression via API");
            }
        }
    };

    // Effect triggers
    useEffect(() => {
        if (activeTab === "backups") fetchBackups();
        else if (activeTab === "files") { fetchFiles(); setSelectedFile(null); setFileContent(""); }
        else if (activeTab === "logs") {
            fetchLogFiles();
            if (selectedLogFile) readLogFile(selectedLogFile);
        }
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
                        <button className="btn btn--sm btn--primary" onClick={() => handleAction("start")} disabled={server.status === "running"}><Play size={16} /> {t("servers.start")}</button>
                        <button className="btn btn--sm btn--secondary" onClick={() => handleAction("restart")} disabled={server.status !== "running"}><RotateCw size={16} /></button>
                        <button className="btn btn--sm btn--danger" onClick={() => handleAction("stop")} disabled={server.status !== "running"}><Square size={16} /></button>
                    </div>
                </div>
            );
        }
    }, [server, setPageTitle, handleAction, t]);

    const configHasChanges = (() => {
        if (!configFormData.id || !initialConfigFormData.id) return false;

        const keys = Array.from(new Set([...Object.keys(configFormData), ...Object.keys(initialConfigFormData)]));

        for (const key of keys) {
            // @ts-ignore
            const val1 = configFormData[key];
            // @ts-ignore
            const val2 = initialConfigFormData[key];

            // Custom equality check
            const areEqual = (v1: any, v2: any) => {
                // Treat null, undefined, empty string as equivalent
                const isEmpty1 = v1 === null || v1 === undefined || v1 === "";
                const isEmpty2 = v2 === null || v2 === undefined || v2 === "";
                if (isEmpty1 && isEmpty2) return true;
                if (isEmpty1 !== isEmpty2) return false;

                // Loose equality
                if (v1 == v2) return true;

                // Deep compare
                if (typeof v1 === "object" && typeof v2 === "object") {
                    if (v1 === null || v2 === null) return v1 === v2; // Should be caught by isEmpty but being safe
                    return JSON.stringify(v1) === JSON.stringify(v2);
                }

                return false;
            };

            if (!areEqual(val1, val2)) {
                return true;
            }
        }
        return false;
    })();

    // Merge real-time players (names) with static player data (op, etc.)
    const onlinePlayers: Player[] = currentPlayersList.map(name => {
        const existing = server?.players?.find(p => p.name === name);
        if (existing) return { ...existing, is_online: true };
        return {
            name,
            is_online: true,
            last_seen: new Date().toISOString(),
            is_op: false,
            is_banned: false,
            is_whitelisted: false
        };
    });

    if (!server) return <div className="loading-screen"><div className="spinner"></div></div>;

    return (
        <div className="server-detail-page">
            <div className="server-header-stats">
                {/* ... Stats Pills (Keep as is, they were fine) ... */}
                <div className="stat-pill"><div className="stat-pill__icon"><Clock size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">UPTIME</div><div className="stat-pill__value">{uptime}</div></div></div>
                <div className="stat-pill"><div className="stat-pill__icon"><Users size={16} /></div><div className="stat-pill__content"><div className="stat-pill__label">PLAYERS</div><div className="stat-pill__value">{currentPlayersList.length} / {server.max_players || 100}</div></div></div>
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
                        onCreateFolder={createFolder}
                        onCreateFile={createFile}
                        onUploadFiles={uploadFiles}
                        onDeleteFile={deleteFile}
                        onRenameFile={renameFile}
                        onCopyFile={copyFile}
                        onMoveFile={moveFile}
                    />
                )}

                {activeTab === "logs" && (
                    <ServerLogs
                        logFiles={logFiles}
                        selectedLogFile={selectedLogFile}
                        logContent={logContent}
                        onSelectLogFile={readLogFile}
                        onRefresh={() => {
                            fetchLogFiles();
                            if (selectedLogFile) readLogFile(selectedLogFile);
                        }}
                    />
                )}

                {activeTab === "config" && (
                    <ServerConfig
                        serverId={id}
                        configFormData={configFormData}
                        configSaving={configSaving}
                        configError={configError}
                        javaVersions={javaVersions}
                        updateConfigValue={updateConfigValue}
                        toggleJvmArg={toggleJvmArg}
                        handleSaveConfig={handleSaveConfig}
                        onDelete={handleDeleteServer}
                        onReinstall={handleReinstallServer}
                        hasChanges={configHasChanges}
                    />
                )}

                {activeTab === "mods" && (
                    <WorkInProgress />
                )}

                {activeTab === "players" && (
                    <ServerPlayers
                        players={onlinePlayers} // Real-time Online players
                        playerList={playerData} // Whitelist/Ban lists
                        activeTab={activePlayerTab}
                        onTabChange={setActivePlayerTab}
                        isLoading={false}
                        onAction={handlePlayerAction}
                        onAddPlayer={handleAddPlayer}
                        onRemovePlayer={handleRemovePlayer}
                        onRefresh={() => {
                            fetchServer();
                            fetchPlayerData();
                        }}
                    />
                )}

                {activeTab === "metrics" && (
                    <ServerMetrics
                        serverId={id!}
                        cpuUsage={cpuUsage}
                        ramUsage={ramUsage}
                        diskUsage={diskUsage}
                        maxHeapBytes={memoryLimit ?? server?.max_heap_bytes}
                        serverStatus={server?.status || "stopped"}
                        currentPlayers={currentPlayers}
                        maxPlayers={server?.max_players || 100}
                    />
                )}

                {activeTab === "webhooks" && (
                    <WorkInProgress />
                )}
            </div>

            <AddPlayerModal
                isOpen={showAddPlayerModal}
                onClose={() => setShowAddPlayerModal(false)}
                onAdd={handleConfirmAddPlayer}
                knownPlayers={server?.players || []}
                title={
                    activePlayerTab === "whitelist" ? "Ajouter à la Whitelist" :
                        activePlayerTab === "ops" ? "Ajouter un Opérateur" :
                            activePlayerTab === "bans" ? "Bannir un joueur" : undefined
                }
            />
        </div>
    );
}
