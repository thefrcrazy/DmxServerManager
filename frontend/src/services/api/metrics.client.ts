import { ClientResponse, BaseClient } from "./base.client";
import { MetricPeriod, MetricsHistory, MetricsHistorySchema } from "@/schemas/operations";
import { queryString } from "./query";

export class MetricsClient extends BaseClient {
    history(serverId: string, period: MetricPeriod): Promise<ClientResponse<MetricsHistory>> {
        return this.request(
            `/servers/${encodeURIComponent(serverId)}/metrics${queryString({ period })}`,
            MetricsHistorySchema,
        );
    }
}
