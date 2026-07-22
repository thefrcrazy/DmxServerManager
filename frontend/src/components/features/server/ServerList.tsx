import { Download, Play, RotateCw, Skull, Square } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import type { ConnectionInfo, GameProfile, Instance } from "@/schemas/api";
import type { CurrentServerMetric } from "@/schemas/operations";
import type { ServerAction } from "@/services/api/server.client";
import { Table, Tooltip } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePermission } from "@/hooks";
import { useToast } from "@/contexts/ToastContext";
import { fallbackGameArtwork, gameProfileVisual } from "@/constants/gameProfiles";
import ServerCard from "./ServerCard";
import { apiService } from "@/services";
import MaskedConnection from "./MaskedConnection";
import ServerResourceUsage from "./ServerResourceUsage";

interface ServerListProps {
    servers: Instance[];
    profiles: GameProfile[];
    viewMode: "grid" | "list";
    onAction: (id: string, action: ServerAction) => Promise<boolean | void>;
    metrics: Record<string, CurrentServerMetric>;
}

function actionPermission(action: ServerAction): string {
    if (action === "install") return "server.update_game";
    if (action === "restart") return "server.start";
    return `server.${action}`;
}

export default function ServerList({ servers, profiles, viewMode, onAction, metrics }: ServerListProps) {
    const { t } = useLanguage();
    const toast = useToast();
    const { hasPermission } = usePermission();
    const [loadingAction, setLoadingAction] = useState<string | null>(null);
    const [playerCounts, setPlayerCounts] = useState<Record<string, number>>({});
    const [connections, setConnections] = useState<Record<string, ConnectionInfo>>({});
    const profilesById = useMemo(() => new Map(profiles.map((profile) => [profile.id, profile])), [profiles]);

    useEffect(() => {
        let active = true;
        void Promise.all(servers.map(async (server) => {
            const [players, connection] = await Promise.all([
                apiService.players.snapshot(server.id),
                apiService.servers.getConnection(server.id),
            ]);
            return { id: server.id, players: players.success ? players.data.online_count : undefined, connection: connection.success ? connection.data : undefined };
        })).then((entries) => {
            if (!active) return;
            setPlayerCounts(Object.fromEntries(entries.filter((entry) => entry.players !== undefined).map((entry) => [entry.id, entry.players!] as const)));
            setConnections(Object.fromEntries(entries.filter((entry) => entry.connection !== undefined).map((entry) => [entry.id, entry.connection!] as const)));
        });
        return () => { active = false; };
    }, [servers]);

    const run = async (id: string, action: ServerAction) => {
        if (loadingAction) return;
        if (!hasPermission(actionPermission(action)) || (action === "restart" && !hasPermission("server.stop"))) {
            toast.error(t("common.no_permission"));
            return;
        }
        setLoadingAction(`${id}-${action}`);
        const succeeded = await onAction(id, action);
        if (succeeded) toast.success(t(`servers.action_${action}_success`));
        else toast.error(t("servers.action_failed"));
        setLoadingAction(null);
    };

    if (viewMode === "grid") {
        return (
            <div className="server-grid">
                {servers.map((server) => <ServerCard
                    key={server.id}
                    server={server}
                    capabilities={new Set(profilesById.get(server.profile_id)?.capabilities ?? [])}
                    playerCount={playerCounts[server.id]}
                    connection={connections[server.id]}
                    metric={metrics[server.id]}
                    onAction={(id, action) => void run(id, action)}
                />)}
            </div>
        );
    }

    return (
        <Table className="server-list-table">
            <thead><tr>
                <th>{t("servers.server_header")}</th><th>{t("servers.status")}</th><th>{t("servers.players")}</th><th>{t("metrics.resources")}</th><th>{t("servers.installed_version")}</th><th>{t("servers.connection")}</th><th><span className="sr-only">{t("servers.actions")}</span></th>
            </tr></thead>
            <tbody>{servers.map((server) => {
                const running = server.runtime_state === "running";
                const needsInstall = ["not_installed", "failed"].includes(server.installation_state);
                const busy = loadingAction?.startsWith(`${server.id}-`) ?? false;
                const profile = profilesById.get(server.profile_id);
                const capabilities = new Set(profile?.capabilities ?? []);
                const visual = gameProfileVisual(server.profile_id, profile?.name);
                const canInstall = capabilities.has("install") && hasPermission("server.update_game");
                const canStartStop = capabilities.has("lifecycle");
                return (
                    <tr key={server.id}>
                        <td>
                            <Link className="server-list-table__identity" to={`/servers/${server.id}`}>
                                <img
                                    className="server-game-thumb"
                                    src={visual.artwork}
                                    alt=""
                                    loading="lazy"
                                    referrerPolicy="no-referrer"
                                    style={{ objectPosition: visual.artworkPosition }}
                                    onError={(event) => fallbackGameArtwork(event, visual.fallbackArtwork)}
                                />
                                <span className="server-list-table__identity-copy">
                                    <strong>{server.name}</strong>
                                    <small>{visual.label}</small>
                                </span>
                            </Link>
                        </td>
                        <td>
                            <span className={`badge badge--${running ? "success" : server.runtime_state === "crashed" ? "danger" : needsInstall ? "warning" : "info"}`}>{needsInstall ? t(`servers.installation_states.${server.installation_state}`) : t(`servers.runtime_states.${server.runtime_state}`)}</span>
                        </td>
                        <td className="server-list-table__players">{playerCounts[server.id] ?? "—"}</td>
                        <td><ServerResourceUsage metric={metrics[server.id]} running={running} compact /></td>
                        <td><code className="server-list-table__version">{server.installed_version ?? server.installed_build ?? "—"}</code></td>
                        <td><MaskedConnection connection={connections[server.id]} compact /></td>
                        <td><div className="server-actions">
                            {needsInstall && canInstall ? (
                                <button className="btn btn--secondary server-list-table__primary-action" disabled={busy} onClick={() => void run(server.id, "install")}><Download size={16} aria-hidden="true" /><span>{t("servers.install")}</span></button>
                            ) : running && canStartStop ? <>
                                {hasPermission("server.start") && hasPermission("server.stop") && <Tooltip content={t("servers.restart")} position="top"><button aria-label={t("servers.restart")} className="btn btn--icon btn--ghost" disabled={busy} onClick={() => void run(server.id, "restart")}><RotateCw size={17} /></button></Tooltip>}
                                {hasPermission("server.stop") && <Tooltip content={t("servers.stop")} position="top"><button aria-label={t("servers.stop")} className="btn btn--icon btn--ghost" disabled={busy} onClick={() => void run(server.id, "stop")}><Square size={17} /></button></Tooltip>}
                                {hasPermission("server.kill") && <Tooltip content={t("servers.kill")} position="top"><button aria-label={t("servers.kill")} className="btn btn--icon btn--ghost text-danger" disabled={busy} onClick={() => void run(server.id, "kill")}><Skull size={17} /></button></Tooltip>}
                            </> : !running && canStartStop && hasPermission("server.start") && server.installation_state === "installed" ? (
                                <button className="btn btn--success server-list-table__primary-action" disabled={busy || server.installation_state !== "installed"} onClick={() => void run(server.id, "start")}><Play size={16} aria-hidden="true" /><span>{t("servers.start")}</span></button>
                            ) : <span className="text-muted">—</span>}
                        </div></td>
                    </tr>
                );
            })}</tbody>
        </Table>
    );
}
