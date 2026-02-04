// API Service - Centralized API calls with TypeScript types
import {
    AuthResponse, SetupStatus, Server, Backup,
    AppSettings, MetricsHistoryResponse
} from '../schemas/api';

// Types locaux pour les réponses API
export interface ApiResponse<T> {
    data: T;
    success: boolean;
    timestamp: string;
    error?: ApiError;
}

export interface ApiError {
    error: string;
    code?: string;
}

const API_BASE_URL = '/api/v1';

interface ApiOptions extends RequestInit {
    skipAuth?: boolean;
}

class ApiService {
    private getToken(): string | null {
        return localStorage.getItem('token');
    }

    private async request<T>(endpoint: string, options: ApiOptions = {}): Promise<ApiResponse<T>> {
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

        try {
            const response = await fetch(`${API_BASE_URL}${endpoint}`, {
                ...fetchOptions,
                headers,
            });

            const timestamp = new Date().toISOString();

            if (!response.ok) {
                const errorData = await response.json().catch(() => ({ error: 'Unknown error' })) as ApiError;
                throw new Error(errorData.error || errorData.code || `HTTP ${response.status}`);
            }

            // Handle empty responses
            const text = await response.text();
            const data = text ? JSON.parse(text) : null;

            return {
                data,
                success: true,
                timestamp
            };
        } catch (error) {
            const timestamp = new Date().toISOString();
            if (error instanceof Error) {
                return {
                    data: null,
                    success: false,
                    timestamp,
                    error: {
                        error: error.message,
                        code: 'API_ERROR'
                    }
                } as any;
            }
            throw error;
        }
    }

    // Auth
    async login(username: string, password: string): Promise<ApiResponse<AuthResponse>> {
        return this.request<AuthResponse>('/auth/login', {
            method: 'POST',
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async register(username: string, password: string): Promise<ApiResponse<AuthResponse>> {
        return this.request<AuthResponse>('/auth/register', {
            method: 'POST',
            body: JSON.stringify({ username, password }),
            skipAuth: true,
        });
    }

    async checkAuthStatus(): Promise<ApiResponse<SetupStatus>> {
        return this.request<SetupStatus>('/auth/status', { skipAuth: true });
    }

    // Servers
    async getServers(): Promise<ApiResponse<Server[]>> {
        return this.request<Server[]>('/servers');
    }

    async getServer(id: string): Promise<ApiResponse<Server>> {
        return this.request<Server>(`/servers/${id}`);
    }

    async createServer(data: Partial<Server>): Promise<ApiResponse<Server>> {
        return this.request<Server>('/servers', {
            method: 'POST',
            body: JSON.stringify(data),
        });
    }

    async updateServer(id: string, data: Partial<Server>): Promise<ApiResponse<Server>> {
        return this.request<Server>(`/servers/${id}`, {
            method: 'PUT',
            body: JSON.stringify(data),
        });
    }

    async deleteServer(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}`, {
            method: 'DELETE',
        });
    }

    async startServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/start`, {
            method: 'POST',
        });
    }

    async stopServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/stop`, {
            method: 'POST',
        });
    }

    async restartServer(id: string): Promise<ApiResponse<{ status: string }>> {
        return this.request<{ status: string }>(`/servers/${id}/restart`, {
            method: 'POST',
        });
    }

    async killServer(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}/kill`, {
            method: 'POST',
        });
    }

    async reinstallServer(id: string): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>(`/servers/${id}/reinstall`, {
            method: 'POST',
        });
    }

    async sendCommand(id: string, command: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/servers/${id}/command`, {
            method: 'POST',
            body: JSON.stringify({ command }),
        });
    }

    async getServerMetrics(id: string, period: string = '1d'): Promise<ApiResponse<MetricsHistoryResponse>> {
        return this.request<MetricsHistoryResponse>(`/servers/${id}/metrics?period=${period}`);
    }

    // Backups
    async getBackups(serverId?: string): Promise<ApiResponse<Backup[]>> {
        const query = serverId ? `?server_id=${serverId}` : '';
        return this.request<Backup[]>(`/backups${query}`);
    }

    async createBackup(serverId: string): Promise<ApiResponse<Backup>> {
        return this.request<Backup>('/backups', {
            method: 'POST',
            body: JSON.stringify({ server_id: serverId }),
        });
    }

    async deleteBackup(id: string): Promise<ApiResponse<{ success: boolean }>> {
        return this.request<{ success: boolean }>(`/backups/${id}`, {
            method: 'DELETE',
        });
    }

    async restoreBackup(id: string): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>(`/backups/${id}/restore`, {
            method: 'POST',
        });
    }

    // Settings
    async getSettings(): Promise<ApiResponse<AppSettings>> {
        return this.request<AppSettings>('/settings');
    }

    async updateSettings(data: Partial<AppSettings>): Promise<ApiResponse<{ success: boolean; message: string }>> {
        return this.request<{ success: boolean; message: string }>(`/settings`, {
            method: 'PUT',
            body: JSON.stringify(data),
        });
    }
}

export const apiService = new ApiService();
export default apiService;
