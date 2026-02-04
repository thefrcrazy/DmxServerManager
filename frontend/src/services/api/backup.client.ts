import { BaseClient, ApiResponse } from "./base.client";
import { Backup } from "../../schemas/api";

export class BackupClient extends BaseClient {
    async getBackups(serverId?: string): Promise<ApiResponse<Backup[]>> {
        const query = serverId ? `?server_id=${serverId}` : "";
        return this.request<Backup[]>(`/backups${query}`);
    }

    async createBackup(serverId: string): Promise<ApiResponse<Backup>> {
        return this.request<Backup>("/backups", {
            method: "POST",
            body: JSON.stringify({ server_id: serverId }),
        });
    }

    async deleteBackup(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/backups/${id}`, {
            method: "DELETE",
        });
    }

    async restoreBackup(id: string): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>(`/backups/${id}/restore`, {
            method: "POST",
        });
    }
}
