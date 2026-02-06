import { useState, useRef, useEffect, useCallback } from "react";

interface WebSocketMetrics {
    cpu: number;
    cpu_normalized?: number;
    memory: number;
    memory_limit?: number;
    disk_bytes?: number;
    players?: number;
    players_list?: string[];
}

interface UseServerWebSocketReturn {
    logs: string[];
    setLogs: React.Dispatch<React.SetStateAction<string[]>>;
    isConnected: boolean;
    cpuUsage: number;
    ramUsage: number;
    memoryLimit: number | null;
    diskUsage: number | null;
    currentPlayers: number;
    currentPlayersList: string[];
    startTime: Date | null;
    setStartTime: React.Dispatch<React.SetStateAction<Date | null>>;
    isInstalling: boolean;
    setIsInstalling: React.Dispatch<React.SetStateAction<boolean>>;
    isAuthRequired: boolean;
    setIsAuthRequired: React.Dispatch<React.SetStateAction<boolean>>;
    isBooted: boolean;
    setIsBooted: React.Dispatch<React.SetStateAction<boolean>>;
    sendCommand: (command: string) => void;
    clearLogs: () => void;
    wsRef: React.RefObject<WebSocket | null>;
}


interface UseServerWebSocketOptions {
    serverId: string | undefined;
    serverStatus: string | undefined;
    onServerUpdate: () => void;
    onStatusChange?: (status: string) => void;
}

export function useServerWebSocket({
    serverId,
    serverStatus,
    onServerUpdate,
    onStatusChange,
}: UseServerWebSocketOptions): UseServerWebSocketReturn {
    const [logs, setLogs] = useState<string[]>([]);
    const [isConnected, setIsConnected] = useState(false);
    const [startTime, setStartTime] = useState<Date | null>(null);
    const [cpuUsage, setCpuUsage] = useState(0);
    const [ramUsage, setRamUsage] = useState(0);
    const [memoryLimit, setMemoryLimit] = useState<number | null>(null);
    const [diskUsage, setDiskUsage] = useState<number | null>(null);
    const [currentPlayers, setCurrentPlayers] = useState(0);
    const [currentPlayersList, setCurrentPlayersList] = useState<string[]>([]);
    const [isInstalling, setIsInstalling] = useState(false);
    const [isAuthRequired, setIsAuthRequired] = useState(false);
    const [isBooted, setIsBooted] = useState(false);

    const wsRef = useRef<WebSocket | null>(null);
    const reconnectTimeoutRef = useRef<NodeJS.Timeout | null>(null);
    const retryCountRef = useRef(0);
    const shouldReconnectRef = useRef(true);
    const serverStatusRef = useRef(serverStatus);

    // Fetch console log history
    const fetchConsoleLog = useCallback(async () => {
        if (!serverId) return;
        try {
            // Try install log first
            const installRes = await fetch(`/api/v1/servers/${serverId}/files/read?path=logs/install.log`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });

            if (installRes.ok) {
                const data = await installRes.json();
                // Just check for content, don't parse for status
                if (data.content && serverStatusRef.current === "installing") {
                    // If server status is installing, show install logs
                    // But we might want to append? For now let's just respect the current logic priority
                }
            }

            // Try console log
            const res = await fetch(`/api/v1/servers/${serverId}/files/read?path=logs/console.log`, {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });

            if (res.ok) {
                const data = await res.json();
                if (data.content && data.content.length > 0) {
                    setLogs(data.content.split("\n"));
                }
            } else if (installRes.ok) {
                // If no console log but install log exists, use that
                const data = await installRes.json();
                if (data.content) setLogs(data.content.split("\n"));
            }
        } catch (error) {
            console.error("Failed to fetch console log:", error);
        }
    }, [serverId]); // Removed serverStatus dependence

    // WebSocket connection
    const connectWebSocket = useCallback(() => {
        if (!serverId) return;

        const token = localStorage.getItem("token");
        if (!token) return; // Don't even try without token

        if (wsRef.current) {
            if (wsRef.current.readyState === WebSocket.OPEN ||
                wsRef.current.readyState === WebSocket.CONNECTING) return;
            wsRef.current.onclose = null;
            wsRef.current.close();
        }

        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        const wsUrl = `${protocol}//${window.location.host}/api/v1/ws/console/${serverId}?token=${encodeURIComponent(token)}`;
        
        // Only use URL param, avoid protocol abuse which can cause issues
        const ws = new WebSocket(wsUrl);

        ws.onopen = () => {
            setIsConnected(true);
            retryCountRef.current = 0;
            fetchConsoleLog();
        };

        ws.onmessage = (event) => {
            const message = event.data;

            // Handle status updates
            if (message.startsWith("[STATUS]:")) {
                const status = message.replace("[STATUS]:", "").trim();

                // Update internal states based on status
                if (status === "running") {
                    setStartTime(new Date());
                    setIsInstalling(false);
                    setIsAuthRequired(false);
                } else if (status === "booted") {
                    setIsBooted(true);
                } else if (status === "auth_success") {
                    setIsAuthRequired(false);
                    setIsInstalling(false);
                } else if (status === "installing") {
                    setIsInstalling(true);
                    setIsAuthRequired(false);
                } else if (status === "auth_required") {
                    setIsAuthRequired(true);
                } else if (status === "stopped" || status === "offline") {
                    setStartTime(null);
                    setIsInstalling(false);
                    setIsAuthRequired(false);
                    setIsBooted(false);
                }

                onStatusChange?.(status);
                onServerUpdate();
                return;
            }

            // Handle metrics
            if (message.trim().startsWith("[METRICS]:")) {
                try {
                    const metrics: WebSocketMetrics = JSON.parse(message.trim().substring(10));
                    // Use cpu_normalized (0-100%) instead of raw cpu (0-800% on 8 cores)
                    setCpuUsage(metrics.cpu_normalized ?? metrics.cpu ?? 0);
                    setRamUsage(metrics.memory || 0);
                    setMemoryLimit(metrics.memory_limit ?? null);
                    setCurrentPlayers(metrics.players || 0);
                    setCurrentPlayersList(metrics.players_list || []);
                    if (metrics.disk_bytes !== undefined) setDiskUsage(metrics.disk_bytes);
                } catch (e) {
                    console.error("Failed to parse metrics", e);
                }
                return;
            }

            setLogs((prev) => {
                const newLogs = [...prev, message];
                if (newLogs.length > 500) return newLogs.slice(-500);
                return newLogs;
            });
        };

        ws.onclose = () => {
            setIsConnected(false);
            wsRef.current = null;
            const shouldRetry = shouldReconnectRef.current &&
                (serverStatusRef.current === "running" ||
                    serverStatusRef.current === "installing" ||
                    serverStatusRef.current === "auth_required");
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
    }, [serverId, fetchConsoleLog, onServerUpdate, onStatusChange]);

    // Update server status ref and handle connection
    useEffect(() => {
        serverStatusRef.current = serverStatus;

        setIsInstalling(serverStatus === "installing");
        setIsAuthRequired(serverStatus === "auth_required");

        const isActivating = (serverStatus === "running" ||
            serverStatus === "installing" ||
            serverStatus === "auth_required" ||
            serverStatus === "starting");

        if (isActivating) {
            shouldReconnectRef.current = true;
            // If status changed to something active, force a new connection if none exists or if it's closed
            if (!wsRef.current || wsRef.current.readyState === WebSocket.CLOSED || wsRef.current.readyState === WebSocket.CLOSING) {
                connectWebSocket();
            }
        }

        if (serverStatus === "stopped" || serverStatus === "offline" || serverStatus === "missing") {
            setIsAuthRequired(false);
            setIsInstalling(false);
            shouldReconnectRef.current = false;
            if (wsRef.current) {
                wsRef.current.close();
                wsRef.current = null;
            }
        }
    }, [serverStatus, connectWebSocket]);

    // Refetch logs when status changes (without reconnecting WS if already active)
    useEffect(() => {
        if (serverStatus === "running" || serverStatus === "installing") {
            fetchConsoleLog();
        }
    }, [serverStatus, fetchConsoleLog]);

    // Initial connection and cleanup
    useEffect(() => {
        setLogs([]);
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
    }, [serverId, fetchConsoleLog]);

    // Send command to server
    const sendCommand = useCallback((command: string) => {
        if (command.trim() && wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(command);
            setLogs((prev) => [...prev, `> ${command}`]);
        }
    }, []);

    // Clear logs
    const clearLogs = useCallback(() => {
        setLogs([]);
    }, []);

    return {
        logs,
        setLogs,
        isConnected,
        cpuUsage,
        ramUsage,
        memoryLimit,
        diskUsage,
        currentPlayers,
        currentPlayersList,
        startTime,
        setStartTime,
        isInstalling,
        setIsInstalling,
        isAuthRequired,
        setIsAuthRequired,
        isBooted,
        setIsBooted,
        sendCommand,
        clearLogs,
        wsRef,
    };
}
