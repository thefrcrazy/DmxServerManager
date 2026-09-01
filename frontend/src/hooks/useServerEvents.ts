import { Dispatch, SetStateAction, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EventEnvelopeSchema, JobSchema, RuntimeStateSchema } from "@/schemas/api";
import {
    BedrockArchiveAuthorization,
    BedrockArchiveAuthorizationSchema,
    HytaleDeviceAuthorization,
    HytaleDeviceAuthorizationSchema,
} from "@/schemas/operations";
import { API_BASE_URL } from "@/services/api/base.client";
import { apiService } from "@/services";
import type { ServerLogSource } from "@/services/api/server.client";

const MAX_VISIBLE_CONSOLE_LOG_LINES = 1_000;
const MAX_VISIBLE_INSTALL_LOG_LINES = 10_000;
const RECONNECT_MIN_DELAY_MS = 1_000;
const RECONNECT_MAX_DELAY_MS = 30_000;

interface UseServerEventsOptions {
    serverId: string | undefined;
    serverStatus: string | undefined;
    logSource: ServerLogSource;
    onServerUpdate: () => void;
    onStatusChange?: (status: string) => void;
}

interface UseServerEventsReturn {
    logs: string[];
    setLogs: Dispatch<SetStateAction<string[]>>;
    isConnected: boolean;
    sendCommand: (command: string) => Promise<boolean>;
    clearLogs: () => void;
    operationRevision: number;
    playerRevision: number;
    scheduleRevision: number;
    pendingDeviceAuthorization: HytaleDeviceAuthorization | null;
    pendingBedrockArchive: BedrockArchiveAuthorization | null;
    clearPendingBedrockArchive: () => void;
}

function formatLogLine(stream: string, message: string): string {
    return stream === "stderr" || stream.endsWith("_error") ? `[stderr] ${message}` : message;
}

function visibleLogLimit(source: ServerLogSource): number {
    return source === "install" ? MAX_VISIBLE_INSTALL_LOG_LINES : MAX_VISIBLE_CONSOLE_LOG_LINES;
}

/** Séparateur impossible à confondre avec une ligne de journal. */
const OVERLAP_SENTINEL = Symbol("overlap");

/**
 * Longueur du plus long suffixe de `history` égal à un préfixe de `live`.
 *
 * Calcul de bordure KMP, en O(n+m). La version précédente comparait des tranches
 * décroissantes : avec 10 000 lignes d'historique et autant en direct, cela
 * représentait jusqu'à 50 millions de comparaisons et 10 000 allocations de
 * tableau sur le thread principal, à chaque reconnexion du flux.
 */
function longestOverlap(history: string[], live: string[]): number {
    const maxOverlap = Math.min(history.length, live.length);
    if (maxOverlap === 0) return 0;
    const combined: Array<string | symbol> = [
        ...live.slice(0, maxOverlap),
        OVERLAP_SENTINEL,
        ...history.slice(-maxOverlap),
    ];
    const border = new Array<number>(combined.length).fill(0);
    for (let index = 1; index < combined.length; index += 1) {
        let length = border[index - 1]!;
        while (length > 0 && combined[index] !== combined[length]) length = border[length - 1]!;
        if (combined[index] === combined[length]) length += 1;
        border[index] = length;
    }
    return border[border.length - 1]!;
}

export function mergeLogHistory(history: string[], live: string[], limit: number): string[] {
    if (live.length === 0) return history.slice(-limit);
    return [...history, ...live.slice(longestOverlap(history, live))].slice(-limit);
}

export function useServerEvents({ serverId, serverStatus, logSource, onServerUpdate, onStatusChange }: UseServerEventsOptions): UseServerEventsReturn {
    const [logs, setLogs] = useState<string[]>([]);
    const [isConnected, setIsConnected] = useState(false);
    const [operationRevision, setOperationRevision] = useState(0);
    const [playerRevision, setPlayerRevision] = useState(0);
    const [scheduleRevision, setScheduleRevision] = useState(0);
    const [pendingDeviceAuthorization, setPendingDeviceAuthorization] = useState<HytaleDeviceAuthorization | null>(null);
    const [pendingBedrockArchive, setPendingBedrockArchive] = useState<BedrockArchiveAuthorization | null>(null);
    const historyRequest = useRef(0);
    // Les lignes reçues sont regroupées et appliquées une fois par trame. Un
    // `setLogs` par ligne provoquait, pendant une installation SteamCMD qui en
    // émet des centaines par seconde, deux allocations de tableau de 10 000
    // éléments et un rendu complet de la console à chaque ligne.
    const pendingLogs = useRef<string[]>([]);
    const flushFrame = useRef<number | null>(null);

    const flushLogs = useCallback(() => {
        flushFrame.current = null;
        const pending = pendingLogs.current;
        if (pending.length === 0) return;
        pendingLogs.current = [];
        setLogs((current) => [...current, ...pending].slice(-visibleLogLimit(logSource)));
    }, [logSource]);

    const appendLog = useCallback((line: string) => {
        pendingLogs.current.push(line);
        if (flushFrame.current !== null) return;
        flushFrame.current = requestAnimationFrame(flushLogs);
    }, [flushLogs]);

    const clearLogs = useCallback(() => {
        pendingLogs.current = [];
        setLogs([]);
    }, []);

    const clearPendingBedrockArchive = useCallback(() => setPendingBedrockArchive(null), []);

    useEffect(() => () => {
        if (flushFrame.current !== null) cancelAnimationFrame(flushFrame.current);
    }, []);

    const applyEvent = useCallback((event: MessageEvent<string>) => {
        let raw: unknown;
        try { raw = JSON.parse(event.data); } catch { return; }
        const parsed = EventEnvelopeSchema.safeParse(raw);
        if (!parsed.success || (parsed.data.server_id && parsed.data.server_id !== serverId)) return;

        const { type, payload } = parsed.data;
        if (type === "job.waiting_for_user") {
            const authorization = HytaleDeviceAuthorizationSchema.safeParse(payload);
            if (authorization.success) {
                setPendingDeviceAuthorization(authorization.data);
                setPendingBedrockArchive(null);
            }
            const archive = BedrockArchiveAuthorizationSchema.safeParse(payload);
            if (archive.success && archive.data.interaction.instance_id === serverId) {
                setPendingBedrockArchive(archive.data);
                setPendingDeviceAuthorization(null);
            }
        }
        if (type === "job.updated") {
            const job = JobSchema.safeParse(payload);
            if (job.success && job.data.state !== "waiting_for_user") {
                setPendingDeviceAuthorization((current) => current?.job_id === job.data.id ? null : current);
                setPendingBedrockArchive((current) => current?.job_id === job.data.id ? null : current);
            }
        }
        if (type === "server.log" && typeof payload === "object" && payload !== null && "message" in payload) {
            const { message, stream } = payload as { message?: unknown; stream?: unknown };
            const isRequestedSource = typeof stream !== "string"
                || (logSource === "install" ? stream.startsWith("install") : !stream.startsWith("install"));
            if (typeof message === "string" && isRequestedSource) {
                appendLog(formatLogLine(typeof stream === "string" ? stream : "", message));
            }
            return;
        }

        if (typeof payload === "object" && payload !== null) {
            const status = "runtime_state" in payload ? (payload as { runtime_state?: unknown }).runtime_state
                : "status" in payload ? (payload as { status?: unknown }).status : undefined;
            const validStatus = RuntimeStateSchema.safeParse(status);
            if (validStatus.success) onStatusChange?.(validStatus.data);
        }
        if (type.startsWith("job.") || type.startsWith("backup.") || type.startsWith("file.") || type.startsWith("config.") || type.startsWith("mod.") || type.startsWith("schedule.") || type === "server.metrics" || type === "server.players") {
            setOperationRevision((revision) => revision + 1);
        }
        if (type === "server.players") setPlayerRevision((revision) => revision + 1);
        if (type.startsWith("schedule.")) setScheduleRevision((revision) => revision + 1);
        if (type.startsWith("server.") || type.startsWith("job.")) onServerUpdate();
    }, [appendLog, logSource, onServerUpdate, onStatusChange, serverId]);

    const loadHistory = useCallback(async () => {
        if (!serverId) return;
        const request = ++historyRequest.current;
        const response = await apiService.servers.getLogHistory(serverId, logSource);
        if (!response.success || request !== historyRequest.current) return;
        const history = response.data.items.map(({ stream, message }) => formatLogLine(stream, message));
        // Installation and startup output can arrive while the REST history is
        // in flight. Merge instead of replacing so the initial synchronization
        // never erases fresh SSE lines.
        setLogs((current) => mergeLogHistory(history, current, visibleLogLimit(logSource)));
    }, [logSource, serverId]);

    const resynchronize = useCallback(() => {
        void loadHistory();
        setPendingDeviceAuthorization(null);
        setPendingBedrockArchive(null);
        setOperationRevision((revision) => revision + 1);
        setPlayerRevision((revision) => revision + 1);
        onServerUpdate();
    }, [loadHistory, onServerUpdate]);

    useEffect(() => {
        setLogs([]);
        void loadHistory();
    }, [loadHistory]);

    useEffect(() => {
        if (!serverId) return;
        const source = new EventSource(`${API_BASE_URL}/events?server_id=${encodeURIComponent(serverId)}`, { withCredentials: true });
        // `EventSource` se reconnecte seul toutes les trois secondes environ et
        // déclenche `onerror` à chaque tentative. Rattraper l'historique sans
        // délai à chaque échec martelait deux points d'entrée REST indéfiniment
        // tant que le serveur restait indisponible.
        let retryDelay = RECONNECT_MIN_DELAY_MS;
        let retryTimer: number | null = null;
        source.onopen = () => {
            setIsConnected(true);
            retryDelay = RECONNECT_MIN_DELAY_MS;
            if (retryTimer !== null) {
                window.clearTimeout(retryTimer);
                retryTimer = null;
            }
        };
        source.onerror = () => {
            setIsConnected(false);
            if (retryTimer !== null) return;
            retryTimer = window.setTimeout(() => {
                retryTimer = null;
                retryDelay = Math.min(retryDelay * 2, RECONNECT_MAX_DELAY_MS);
                void loadHistory();
                onServerUpdate();
            }, retryDelay);
        };
        source.onmessage = applyEvent;
        for (const type of [
            "server.log", "server.updated", "server.state", "server.started", "server.stopped", "server.crashed",
            "server.metrics", "server.players", "server.update_applied", "server.update_failed", "server.update_rolled_back",
            "job.updated", "job.waiting_for_user",
            "backup.created", "backup.deleted", "backup.restored", "backup.failed", "backup.restore_failed",
            "file.uploaded", "file.text_written", "file.directory_created", "file.deleted",
            "config.queued", "config.cancelled", "config.applied", "config.conflict",
            "mod.installed", "mod.deleted",
            "schedule.created", "schedule.updated", "schedule.deleted", "schedule.triggered",
        ]) {
            source.addEventListener(type, applyEvent as EventListener);
        }
        source.addEventListener("stream.reset", resynchronize);
        source.addEventListener("stream.lagged", resynchronize);
        return () => {
            if (retryTimer !== null) window.clearTimeout(retryTimer);
            source.close();
        };
    }, [applyEvent, loadHistory, onServerUpdate, resynchronize, serverId]);

    useEffect(() => {
        if (serverStatus !== "running") {
            setLogs((current) => current.slice(-visibleLogLimit(logSource)));
        }
    }, [logSource, serverStatus]);

    useEffect(() => {
        setPendingDeviceAuthorization(null);
        setPendingBedrockArchive(null);
    }, [serverId]);

    const sendCommand = useCallback(async (command: string) => {
        if (!serverId || !command.trim()) return false;
        const response = await apiService.servers.sendCommand(serverId, command.trim());
        if (!response.success) return false;
        appendLog(`> ${command.trim()}`);
        return true;
    }, [appendLog, serverId]);

    return useMemo(() => ({
        logs,
        setLogs,
        isConnected,
        sendCommand,
        clearLogs,
        operationRevision,
        playerRevision,
        scheduleRevision,
        pendingDeviceAuthorization,
        pendingBedrockArchive,
        clearPendingBedrockArchive,
    }), [
        clearLogs,
        clearPendingBedrockArchive,
        isConnected,
        logs,
        operationRevision,
        pendingBedrockArchive,
        pendingDeviceAuthorization,
        playerRevision,
        scheduleRevision,
        sendCommand,
    ]);
}
