import { ClientResponse, BaseClient } from "./base.client";
import {
    CurrentServerMetrics,
    CurrentServerMetricsSchema,
    MetricPeriod,
    MetricsHistory,
    MetricsHistorySchema,
    SystemMetricsSnapshot,
    SystemMetricsSnapshotSchema,
} from "@/schemas/operations";
import { queryString } from "./query";

export class MetricsClient extends BaseClient {
    current(): Promise<ClientResponse<CurrentServerMetrics>> {
        return this.request("/metrics/current", CurrentServerMetricsSchema);
    }

    system(): Promise<ClientResponse<SystemMetricsSnapshot>> {
        return this.request("/metrics/system", SystemMetricsSnapshotSchema);
    }

    history(serverId: string, period: MetricPeriod): Promise<ClientResponse<MetricsHistory>> {
        return this.request(
            `/servers/${encodeURIComponent(serverId)}/metrics${queryString({ period })}`,
            MetricsHistorySchema,
        );
    }
}
