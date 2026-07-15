import { z } from "zod";
import { Job, JobSchema } from "@/schemas/api";
import { BaseClient, ClientResponse } from "./base.client";

export class JobsClient extends BaseClient {
    list(): Promise<ClientResponse<Job[]>> {
        return this.request("/jobs", z.array(JobSchema));
    }

    get(id: string): Promise<ClientResponse<Job>> {
        return this.request(`/jobs/${encodeURIComponent(id)}`, JobSchema);
    }

    cancel(id: string): Promise<ClientResponse<Job>> {
        return this.request(`/jobs/${encodeURIComponent(id)}/cancel`, JobSchema, { method: "POST" });
    }
}
