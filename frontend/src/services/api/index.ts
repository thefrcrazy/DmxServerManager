import { AdminClient } from "./admin.client";
import { AuthClient } from "./auth.client";
import { BackupsClient } from "./backups.client";
import { ChatClient } from "./chat.client";
import { ConfigClient } from "./config.client";
import { CatalogClient } from "./catalog.client";
import { FilesClient } from "./files.client";
import { ImportsClient } from "./imports.client";
import { JobsClient } from "./jobs.client";
import { MetricsClient } from "./metrics.client";
import { ModsClient } from "./mods.client";
import { NotificationsClient } from "./notifications.client";
import { PlayersClient } from "./players.client";
import { ProfileClient } from "./profile.client";
import { ReleasesClient } from "./releases.client";
import { ServerClient } from "./server.client";
import { SchedulesClient } from "./schedules.client";
import { SystemClient } from "./system.client";
import { WebhooksClient } from "./webhooks.client";

class ApiService {
    readonly admin = new AdminClient();
    readonly auth = new AuthClient();
    readonly backups = new BackupsClient();
    readonly chat = new ChatClient();
    readonly config = new ConfigClient();
    readonly catalog = new CatalogClient();
    readonly files = new FilesClient();
    readonly imports = new ImportsClient();
    readonly jobs = new JobsClient();
    readonly metrics = new MetricsClient();
    readonly mods = new ModsClient();
    readonly notifications = new NotificationsClient();
    readonly players = new PlayersClient();
    readonly profiles = new ProfileClient();
    readonly releases = new ReleasesClient();
    readonly servers = new ServerClient();
    readonly schedules = new SchedulesClient();
    readonly system = new SystemClient();
    readonly webhooks = new WebhooksClient();
}

export const apiService = new ApiService();
export default apiService;
