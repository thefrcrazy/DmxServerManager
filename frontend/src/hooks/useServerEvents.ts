import { Dispatch, SetStateAction, useCallback, useEffect, useState } from "react";
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

const MAX_VISIBLE_LOG_LINES = 1_000;

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
    scheduleRevision: number;
    pendingDeviceAuthorization: HytaleDeviceAuthorization | null;
    pendingBedrockArchive: BedrockArchiveAuthorization | null;
    clearPendingBedrockArchive: () => void;
}

function formatLogLine(stream: string, message: string): string {
    return stream === "stderr" || stream.endsWith("_error") ? `[stderr] ${message}` : message;
}

export function useServerEvents({ serverId, serverStatus, logSource, onServerUpdate, onStatusChange }: UseServerEventsOptions): UseServerEventsReturn {
    const [logs, setLogs] = useState<string[]>([]);
    const [isConnected, setIsConnected] = useState(false);
    const [operationRevision, setOperationRevision] = useState(0);
    const [scheduleRevision, setScheduleRevision] = useState(0);
    const [pendingDeviceAuthorization, setPendingDeviceAuthorization] = useState<HytaleDeviceAuthorization | null>(null);
    const [pendingBedrockArchive, setPendingBedrockArchive] = useState<BedrockArchiveAuthorization | null>(null);

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
                setLogs((current) => [...current, formatLogLine(typeof stream === "string" ? stream : "", message)].slice(-MAX_VISIBLE_LOG_LINES));
            }
            return;
        }

        if (typeof payload === "object" && payload !== null) {
            const status = "runtime_state" in payload ? (payload as { runtime_state?: unknown }).runtime_state
                : "status" in payload ? (payload as { status?: unknown }).status : undefined;
            const validStatus = RuntimeStateSchema.safeParse(status);
            if (validStatus.success) onStatusChange?.(validStatus.data);
        }
        if (type.startsWith("job.") || type.startsWith("backup.") || type.startsWith("file.") || type.startsWith("mod.") || type.startsWith("schedule.") || type === "server.metrics") {
            setOperationRevision((revision) => revision + 1);
        }
        if (type.startsWith("schedule.")) setScheduleRevision((revision) => revision + 1);
        if (type.startsWith("server.") || type.startsWith("job.")) onServerUpdate();
    }, [logSource, onServerUpdate, onStatusChange, serverId]);

    const loadHistory = useCallback(async () => {
        if (!serverId) return;
        const response = await apiService.servers.getLogHistory(serverId, logSource);
        if (!response.success) return;
        setLogs(response.data.items.map(({ stream, message }) => formatLogLine(stream, message)).slice(-MAX_VISIBLE_LOG_LINES));
    }, [logSource, serverId]);

    const resynchronize = useCallback(() => {
        void loadHistory();
        setPendingDeviceAuthorization(null);
        setPendingBedrockArchive(null);
        onServerUpdate();
    }, [loadHistory, onServerUpdate]);

    useEffect(() => {
        setLogs([]);
        void loadHistory();
    }, [loadHistory]);

    useEffect(() => {
        if (!serverId) return;
        const source = new EventSource(`${API_BASE_URL}/events?server_id=${encodeURIComponent(serverId)}`, { withCredentials: true });
        source.onopen = () => setIsConnected(true);
        source.onerror = () => setIsConnected(false);
        source.onmessage = applyEvent;
        for (const type of [
            "server.log", "server.updated", "server.state", "job.updated", "job.waiting_for_user",
            "server.metrics",
            "backup.created", "backup.deleted", "backup.restored", "backup.failed", "backup.restore_failed",
            "file.uploaded", "file.text_written", "file.directory_created", "file.deleted",
            "mod.installed", "mod.deleted",
            "schedule.created", "schedule.updated", "schedule.deleted", "schedule.triggered",
        ]) {
            source.addEventListener(type, applyEvent as EventListener);
        }
        source.addEventListener("stream.reset", resynchronize);
        source.addEventListener("stream.lagged", resynchronize);
        return () => source.close();
    }, [applyEvent, resynchronize, serverId]);

    useEffect(() => {
        if (serverStatus !== "running") setLogs((current) => current.slice(-MAX_VISIBLE_LOG_LINES));
    }, [serverStatus]);

    useEffect(() => {
        setPendingDeviceAuthorization(null);
        setPendingBedrockArchive(null);
    }, [serverId]);

    const sendCommand = useCallback(async (command: string) => {
        if (!serverId || !command.trim()) return false;
        const response = await apiService.servers.sendCommand(serverId, command.trim());
        if (!response.success) return false;
        setLogs((current) => [...current, `> ${command.trim()}`].slice(-MAX_VISIBLE_LOG_LINES));
        return true;
    }, [serverId]);

    return {
        logs,
        setLogs,
        isConnected,
        sendCommand,
        clearLogs: () => setLogs([]),
        operationRevision,
        scheduleRevision,
        pendingDeviceAuthorization,
        pendingBedrockArchive,
        clearPendingBedrockArchive: () => setPendingBedrockArchive(null),
    };
}
