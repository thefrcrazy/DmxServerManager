import type { Job } from "@/schemas/api";
import {
    ActivityJobsPageSchema,
    ActivitySummarySchema,
    AuditPageSchema,
    type ActivityJobsPage,
    type ActivitySummary,
    type AuditPage,
} from "@/schemas/operations";
import { BaseClient, type ClientResponse } from "./base.client";
import { queryString } from "./query";

export interface ActivityJobFilters {
    cursor?: string;
    limit?: number;
    state?: Job["state"];
    instance_id?: string;
}

export interface AuditFilters {
    before_id?: number;
    limit?: number;
    actor_user_id?: string;
    action?: string;
    resource_type?: string;
    resource_id?: string;
    outcome?: "success" | "denied" | "failure";
    from?: string;
    to?: string;
}

export class ActivityClient extends BaseClient {
    summary(): Promise<ClientResponse<ActivitySummary>> {
        return this.request("/activity/summary", ActivitySummarySchema);
    }

    jobs(filters: ActivityJobFilters = {}): Promise<ClientResponse<ActivityJobsPage>> {
        return this.request(`/activity/jobs${queryString({
            cursor: filters.cursor,
            limit: filters.limit,
            state: filters.state,
            instance_id: filters.instance_id,
        })}`, ActivityJobsPageSchema);
    }

    audit(filters: AuditFilters = {}): Promise<ClientResponse<AuditPage>> {
        return this.request(`/audit${queryString({
            before_id: filters.before_id,
            limit: filters.limit,
            actor_user_id: filters.actor_user_id,
            action: filters.action,
            resource_type: filters.resource_type,
            resource_id: filters.resource_id,
            outcome: filters.outcome,
            from: filters.from,
            to: filters.to,
        })}`, AuditPageSchema);
    }
}
