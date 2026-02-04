import { useState, useRef, useEffect, useCallback } from 'react';

interface WebSocketMetrics {
    cpu: number;
    memory: number;
    disk_bytes?: number;
}

interface UseServerWebSocketReturn {
    logs: string[];
    setLogs: React.Dispatch<React.SetStateAction<string[]>>;
    isConnected: boolean;
    cpuUsage: number;
    ramUsage: number;
    diskUsage: number | null;
    startTime: Date | null;
    setStartTime: React.Dispatch<React.SetStateAction<Date | null>>;
    isInstalling: boolean;
    setIsInstalling: React.Dispatch<React.SetStateAction<boolean>>;
    isAuthRequired: boolean;
    setIsAuthRequired: React.Dispatch<React.SetStateAction<boolean>>;
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
    const [diskUsage, setDiskUsage] = useState<number | null>(null);
    const [isInstalling, setIsInstalling] = useState(false);
    const [isAuthRequired, setIsAuthRequired] = useState(false);

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
                if (data.content) {
                    const lines = data.content.split("\n");
                    const hasStart = lines.some((l: string) =>
                        l.includes("Initialization de l'installation") ||
                        l.includes("Starting Hytale Server Installation")
                    );
                    const hasEnd = lines.some((l: string) =>
                        l.includes("Installation terminée") ||
                        l.includes("Installation finished")
                    );
                    if (hasStart && !hasEnd && serverStatus !== "running") {
                        setIsInstalling(true);
                    }
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
                const data = await installRes.json();
                if (data.content) setLogs(data.content.split("\n"));
            }
        } catch (error) {
            console.error("Failed to fetch console log:", error);
        }
    }, [serverId, serverStatus]);

    // WebSocket connection
    const connectWebSocket = useCallback(() => {
        if (!serverId) return;

        if (wsRef.current) {
            if (wsRef.current.readyState === WebSocket.OPEN ||
                wsRef.current.readyState === WebSocket.CONNECTING) return;
            wsRef.current.onclose = null;
            wsRef.current.close();
        }

        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        const ws = new WebSocket(`${protocol}//${window.location.host}/api/v1/ws/console/${serverId}`);

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
                onStatusChange?.(status);
                if (status === "running") setStartTime(new Date());
                else setStartTime(null);
                onServerUpdate();
                return;
            }

            // Handle metrics
            if (message.trim().startsWith("[METRICS]:")) {
                try {
                    const metrics: WebSocketMetrics = JSON.parse(message.trim().substring(10));
                    setCpuUsage(metrics.cpu || 0);
                    setRamUsage(metrics.memory || 0);
                    if (metrics.disk_bytes !== undefined) setDiskUsage(metrics.disk_bytes);
                } catch (e) {
                    console.error("Failed to parse metrics", e);
                }
                return;
            }

            // Handle installation messages
            if (message.includes("Initialization of installation") ||
                message.includes("Initialization de l'installation")) {
                setIsInstalling(true);
                setIsAuthRequired(false);
            }
            if (message.includes("IMPORTANT") &&
                (message.includes("authentifier") || message.includes("authenticate"))) {
                if (serverStatusRef.current === "running" || serverStatusRef.current === "starting") {
                    setIsAuthRequired(true);
                }
            }
            if (message.includes("Authentication successful!") || message.includes("Success!")) {
                setIsAuthRequired(false);
            }
            if (message.includes("Installation terminée") || message.includes("Installation finished")) {
                setIsInstalling(false);
                onServerUpdate();
            }

            setLogs((prev) => [...prev, message]);
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

        if ((serverStatus === "running" ||
            serverStatus === "installing" ||
            serverStatus === "auth_required") && !wsRef.current) {
            shouldReconnectRef.current = true;
            connectWebSocket();
        }

        if (serverStatus === "stopped" || serverStatus === "offline") {
            setIsAuthRequired(false);
            setIsInstalling(false);
        }
    }, [serverStatus, connectWebSocket]);

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
        diskUsage,
        startTime,
        setStartTime,
        isInstalling,
        setIsInstalling,
        isAuthRequired,
        setIsAuthRequired,
        sendCommand,
        clearLogs,
        wsRef,
    };
}
