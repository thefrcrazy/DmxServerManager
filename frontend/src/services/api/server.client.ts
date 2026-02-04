import { BaseClient, ApiResponse } from "./base.client";
import { Server, MetricsHistoryResponse } from "../../schemas/api";

export class ServerClient extends BaseClient {
    async getServers(): Promise<ApiResponse<Server[]>> {
        return this.request<Server[]>("/servers");
    }

    async getServer(id: string): Promise<ApiResponse<Server>> {
        return this.request<Server>(`/servers/${id}`);
    }

    async createServer(data: Partial<Server>): Promise<ApiResponse<Server>> {
        return this.request<Server>("/servers", {
            method: "POST",
            body: JSON.stringify(data),
        });
    }

    async updateServer(id: string, data: Partial<Server>): Promise<ApiResponse<Server>> {
        return this.request<Server>(`/servers/${id}`, {
            method: "PUT",
            body: JSON.stringify(data),
        });
    }

    async deleteServer(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}`, {
            method: "DELETE",
        });
    }

    async startServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/start`, {
            method: "POST",
        });
    }

    async stopServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/stop`, {
            method: "POST",
        });
    }

    async restartServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/restart`, {
            method: "POST",
        });
    }

    async killServer(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}/kill`, {
            method: "POST",
        });
    }

    async reinstallServer(id: string): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>(`/servers/${id}/reinstall`, {
            method: "POST",
        });
    }

    async sendCommand(id: string, command: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}/command`, {
            method: "POST",
            body: JSON.stringify({ command }),
        });
    }

    async getServerMetrics(id: string, period: string = "1d"): Promise<ApiResponse<MetricsHistoryResponse>> {
        return this.request<MetricsHistoryResponse>(`/servers/${id}/metrics?period=${period}`);
    }
}
