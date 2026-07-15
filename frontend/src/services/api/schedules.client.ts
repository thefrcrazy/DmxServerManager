import { OperationSuccessSchema, Schedule, ScheduleListSchema, ScheduleSchema, CreateSchedule, UpdateSchedule } from "@/schemas/operations";
import { z } from "zod";
import { BaseClient, ClientResponse } from "./base.client";

export class SchedulesClient extends BaseClient {
    async list(instanceId: string): Promise<ClientResponse<Schedule[]>> {
        return this.request(`/schedules?instance_id=${encodeURIComponent(instanceId)}`, ScheduleListSchema);
    }

    async create(input: CreateSchedule): Promise<ClientResponse<Schedule>> {
        return this.request("/schedules", ScheduleSchema, { method: "POST", body: JSON.stringify(input) });
    }

    async update(id: string, input: UpdateSchedule, version: number): Promise<ClientResponse<Schedule>> {
        return this.request(`/schedules/${encodeURIComponent(id)}`, ScheduleSchema, {
            method: "PUT",
            headers: { "If-Match": `"${version}"` },
            body: JSON.stringify(input),
        });
    }

    async delete(id: string): Promise<ClientResponse<z.infer<typeof OperationSuccessSchema>>> {
        return this.request(`/schedules/${encodeURIComponent(id)}`, OperationSuccessSchema, { method: "DELETE" });
    }
}
