import { useState, useEffect, useRef, useCallback } from "react";

interface ConsoleMessage {
    id: number;
    type: "output" | "input" | "error" | "info" | "warning";
    content: string;
    timestamp: Date;
}

interface UseConsoleReturn {
    messages: ConsoleMessage[];
    connected: boolean;
    connecting: boolean;
    error: string | null;
    sendCommand: (command: string) => void;
    clearMessages: () => void;
    reconnect: () => void;
}

export function useConsole(serverId: string): UseConsoleReturn {
    const [messages, setMessages] = useState<ConsoleMessage[]>([]);
    const [connected, setConnected] = useState(false);
    const [connecting, setConnecting] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const wsRef = useRef<WebSocket | null>(null);
    const messageIdRef = useRef(0);
    const reconnectTimeoutRef = useRef<NodeJS.Timeout>();

    const addMessage = useCallback((type: ConsoleMessage["type"], content: string) => {
        const id = ++messageIdRef.current;
        setMessages(prev => [...prev, { id, type, content, timestamp: new Date() }]);
    }, []);

    const connect = useCallback(() => {
        if (wsRef.current?.readyState === WebSocket.OPEN) return;

        setConnecting(true);
        setError(null);

        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        const wsUrl = `${protocol}//${window.location.hostname}:8080/ws/console/${serverId}`;

        const ws = new WebSocket(wsUrl);
        wsRef.current = ws;

        ws.onopen = () => {
            setConnected(true);
            setConnecting(false);
            addMessage("info", "Connecté à la console");
        };

        ws.onmessage = (event) => {
            const data = event.data as string;

            // Detect message type based on content
            let type: ConsoleMessage["type"] = "output";
            if (data.includes("[ERROR]") || data.includes("Exception")) {
                type = "error";
            } else if (data.includes("[WARN]")) {
                type = "warning";
            } else if (data.includes("[INFO]")) {
                type = "info";
            }

            addMessage(type, data);
        };

        ws.onerror = () => {
            setError("Erreur de connexion WebSocket");
            setConnecting(false);
        };

        ws.onclose = () => {
            setConnected(false);
            setConnecting(false);
            addMessage("info", "Déconnecté de la console");

            // Auto-reconnect after 5 seconds
            reconnectTimeoutRef.current = setTimeout(() => {
                if (serverId) connect();
            }, 5000);
        };
    }, [serverId, addMessage]);

    const sendCommand = useCallback((command: string) => {
        if (wsRef.current?.readyState === WebSocket.OPEN) {
            wsRef.current.send(command);
            addMessage("input", `> ${command}`);
        }
    }, [addMessage]);

    const clearMessages = useCallback(() => {
        setMessages([]);
        messageIdRef.current = 0;
    }, []);

    const reconnect = useCallback(() => {
        if (wsRef.current) {
            wsRef.current.close();
        }
        connect();
    }, [connect]);

    useEffect(() => {
        if (serverId) {
            connect();
        }

        return () => {
            if (reconnectTimeoutRef.current) {
                clearTimeout(reconnectTimeoutRef.current);
            }
            if (wsRef.current) {
                wsRef.current.close();
            }
        };
    }, [serverId, connect]);

    return {
        messages,
        connected,
        connecting,
        error,
        sendCommand,
        clearMessages,
        reconnect,
    };
}
