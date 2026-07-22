import { useCallback, useEffect, useState } from "react";
import {
    CurrentServerMetric,
    CurrentServerMetricSchema,
    LiveServerMetricSchema,
    SystemMetricsSnapshot,
    SystemMetricsSnapshotSchema,
} from "@/schemas/operations";
import { apiService } from "@/services";
import { useGlobalEvents } from "./useGlobalEvents";

export function useLiveMetrics() {
    const [systemMetrics, setSystemMetrics] = useState<SystemMetricsSnapshot | null>(null);
    const [serverMetrics, setServerMetrics] = useState<Record<string, CurrentServerMetric>>({});
    const [loading, setLoading] = useState(true);

    const reload = useCallback(async () => {
        const [system, current] = await Promise.all([
            apiService.metrics.system(),
            apiService.metrics.current(),
        ]);
        if (system.success) setSystemMetrics(system.data);
        if (current.success) {
            setServerMetrics(Object.fromEntries(current.data.items.map((metric) => [metric.server_id, metric])));
        }
        setLoading(false);
    }, []);

    useEffect(() => { void reload(); }, [reload]);
    const { isConnected } = useGlobalEvents({
        enabled: true,
        eventTypes: ["system.metrics", "server.metrics"],
        onEvent: (event) => {
            if (event.type === "system.metrics") {
                const parsed = SystemMetricsSnapshotSchema.safeParse(event.payload);
                if (parsed.success) setSystemMetrics(parsed.data);
                return;
            }
            if (event.type !== "server.metrics" || !event.server_id) return;
            const parsed = LiveServerMetricSchema.safeParse(event.payload);
            if (!parsed.success) return;
            const metric = CurrentServerMetricSchema.parse({
                ...parsed.data,
                server_id: event.server_id,
                recorded_at: event.created_at,
            });
            setServerMetrics((current) => ({ ...current, [metric.server_id]: metric }));
        },
        onResynchronize: reload,
    });

    return { systemMetrics, serverMetrics, loading, isConnected, reload };
}
