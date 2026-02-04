import { BaseClient, ApiResponse } from "./base.client";
import { AppSettings, SystemStats } from "../../schemas/api";

export class SystemClient extends BaseClient {
    async getSettings(): Promise<ApiResponse<AppSettings>> {
        return this.request<AppSettings>("/settings");
    }

    async updateSettings(data: Partial<AppSettings>): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>("/settings", {
            method: "PUT",
            body: JSON.stringify(data),
        });
    }

    async getSystemStats(): Promise<ApiResponse<SystemStats>> {
        return this.request<SystemStats>("/system/stats");
    }
}
