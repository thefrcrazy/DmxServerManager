// API Service - Centralized API calls
const API_BASE_URL = '/api/v1';

interface ApiOptions extends RequestInit {
    skipAuth?: boolean;
}

class ApiService {
    private getToken(): string | null {
        return localStorage.getItem('token');
    }

    private async request<T>(endpoint: string, options: ApiOptions = {}): Promise<T> {
        const { skipAuth = false, ...fetchOptions } = options;

        const headers: HeadersInit = {
            'Content-Type': 'application/json',
            ...fetchOptions.headers,
        };

        if (!skipAuth) {
            const token = this.getToken();
            if (token) {
                (headers as Record<string, string>)['Authorization'] = `Bearer ${token}`;
            }
        }

        const response = await fetch(`${API_BASE_URL}${endpoint}`, {
            ...fetchOptions,
            headers,
        });

        if (!response.ok) {
            const error = await response.json().catch(() => ({ error: 'Unknown error' }));
            throw new Error(error.error || error.message || `HTTP ${response.status}`);
        }

        // Handle empty responses
        const text = await response.text();
        return text ? JSON.parse(text) : (null as T);
    }

    // Auth
    async login(username: string, password: string) {
        return this.request<{ token: string; user: any }>('/auth/login', {
            method: 'POST',
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async register(username: string, password: string) {
        return this.request<{ token: string; user: any }>('/auth/register', {
            method: 'POST',
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async checkAuthStatus() {
        return this.request<{ needs_setup: boolean }>('/auth/status', { skipAuth: true });
    }

    // Servers
    async getServers() {
        return this.request<any[]>('/servers');
    }

    async getServer(id: string) {
        return this.request<any>(`/servers/${id}`);
    }

    async createServer(data: any) {
        return this.request<{ id: string }>('/servers', {
            method: 'POST',
            body: JSON.stringify(data),
        });
    }

    async updateServer(id: string, data: any) {
        return this.request<{ success: boolean }>(`/servers/${id}`, {
            method: 'PUT',
            body: JSON.stringify(data),
        });
    }

    async deleteServer(id: string) {
        return this.request<{ success: boolean }>(`/servers/${id}`, {
            method: 'DELETE',
        });
    }

    async startServer(id: string) {
        return this.request<{ status: string }>(`/servers/${id}/start`, {
            method: 'POST',
        });
    }

    async stopServer(id: string) {
        return this.request<{ status: string }>(`/servers/${id}/stop`, {
            method: 'POST',
        });
    }

    async restartServer(id: string) {
        return this.request<{ status: string }>(`/servers/${id}/restart`, {
            method: 'POST',
        });
    }

    async reinstallServer(id: string) {
        return this.request<{ success: boolean; message: string }>(`/servers/${id}/reinstall`, {
            method: 'POST',
        });
    }

    async sendCommand(id: string, command: string) {
        return this.request<{ success: boolean }>(`/servers/${id}/command`, {
            method: 'POST',
            body: JSON.stringify({ command }),
        });
    }

    // Backups
    async getBackups(serverId?: string) {
        const query = serverId ? `?server_id=${serverId}` : '';
        return this.request<any[]>(`/backups${query}`);
    }

    async createBackup(serverId: string) {
        return this.request<any>('/backups', {
            method: 'POST',
            body: JSON.stringify({ server_id: serverId }),
        });
    }

    async deleteBackup(id: string) {
        return this.request<{ success: boolean }>(`/backups/${id}`, {
            method: 'DELETE',
        });
    }

    async restoreBackup(id: string) {
        return this.request<{ success: boolean; message: string }>(`/backups/${id}/restore`, {
            method: 'POST',
        });
    }

    // Settings
    async getSettings() {
        return this.request<{
            version: string;
            servers_dir: string;
            backups_dir: string;
            webhook_url?: string;
        }>('/settings');
    }

    async updateSettings(data: { webhook_url?: string }) {
        return this.request<{ success: boolean; message: string }>('/settings', {
            method: 'PUT',
            body: JSON.stringify(data),
        });
    }
}

export const apiService = new ApiService();
export default apiService;
