import { AuthClient } from "./auth.client";
import { ServerClient } from "./server.client";
import { BackupClient } from "./backup.client";
import { SystemClient } from "./system.client";

class ApiService {
    public auth = new AuthClient();
    public servers = new ServerClient();
    public backups = new BackupClient();
    public system = new SystemClient();

    // Compatibilité descendante (Legacy support)
    // On redirige les méthodes courantes vers les nouveaux clients
    async login(u: string, p: string) { return this.auth.login(u, p); }
    async getServers() { return this.servers.getServers(); }
    async getServer(id: string) { return this.servers.getServer(id); }
    async createServer(d: any) { return this.servers.createServer(d); }
    async updateServer(id: string, d: any) { return this.servers.updateServer(id, d); }
    async deleteServer(id: string) { return this.servers.deleteServer(id); }
    async startServer(id: string) { return this.servers.startServer(id); }
    async stopServer(id: string) { return this.servers.stopServer(id); }
    async restartServer(id: string) { return this.servers.restartServer(id); }
    async killServer(id: string) { return this.servers.killServer(id); }
    async reinstallServer(id: string) { return this.servers.reinstallServer(id); }
    async sendCommand(id: string, c: string) { return this.servers.sendCommand(id, c); }
    async getServerMetrics(id: string, p?: string) { return this.servers.getServerMetrics(id, p); }
    async getBackups(sid?: string) { return this.backups.getBackups(sid); }
    async createBackup(sid: string) { return this.backups.createBackup(sid); }
    async deleteBackup(id: string) { return this.backups.deleteBackup(id); }
    async restoreBackup(id: string) { return this.backups.restoreBackup(id); }
    async getSettings() { return this.system.getSettings(); }
    async updateSettings(d: any) { return this.system.updateSettings(d); }
}

export const apiService = new ApiService();
export default apiService;
