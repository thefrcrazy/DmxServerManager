import { Download, Play, RotateCw, Server as ServerIcon, Skull, Square } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import type { GameProfile, Instance } from "@/schemas/api";
import type { ServerAction } from "@/services/api/server.client";
import { Table, Tooltip } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePermission } from "@/hooks";
import { useToast } from "@/contexts/ToastContext";
import ServerCard from "./ServerCard";

interface ServerListProps {
    servers: Instance[];
    profiles: GameProfile[];
    viewMode: "grid" | "list";
    onAction: (id: string, action: ServerAction) => Promise<boolean | void>;
}

function actionPermission(action: ServerAction): string {
    if (action === "install") return "server.update_game";
    if (action === "restart") return "server.start";
    return `server.${action}`;
}

export default function ServerList({ servers, profiles, viewMode, onAction }: ServerListProps) {
    const { t } = useLanguage();
    const navigate = useNavigate();
    const toast = useToast();
    const { hasPermission } = usePermission();
    const [loadingAction, setLoadingAction] = useState<string | null>(null);

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
            <div className="server-grid" style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(300px, 1fr))", gap: "1.5rem" }}>
                {servers.map((server) => <ServerCard
                    key={server.id}
                    server={server}
                    capabilities={new Set(profiles.find((profile) => profile.id === server.profile_id)?.capabilities ?? [])}
                    onAction={(id, action) => void run(id, action)}
                />)}
            </div>
        );
    }

    return (
        <Table>
            <thead><tr>
                <th>{t("servers.server_header")}</th><th>{t("servers.profile")}</th><th>{t("servers.installation")}</th><th>{t("servers.status")}</th><th>{t("servers.actions")}</th>
            </tr></thead>
            <tbody>{servers.map((server) => {
                const running = server.runtime_state === "running";
                const needsInstall = ["not_installed", "failed"].includes(server.installation_state);
                const busy = loadingAction?.startsWith(`${server.id}-`) ?? false;
                const capabilities = new Set(profiles.find((profile) => profile.id === server.profile_id)?.capabilities ?? []);
                const canInstall = capabilities.has("install") && hasPermission("server.update_game");
                const canStartStop = capabilities.has("lifecycle");
                return (
                    <tr key={server.id} onClick={() => navigate(`/servers/${server.id}`)} style={{ cursor: "pointer" }}>
                        <td><div className="server-name"><ServerIcon size={18} /><span>{server.name}</span></div></td>
                        <td>{server.profile_id} <span className="text-muted">r{server.profile_revision}</span></td>
                        <td>
                            <span className={`badge badge--${server.installation_state === "installed" ? "success" : "warning"}`}>{t(`servers.installation_states.${server.installation_state}`)}</span>
                            {server.installed_version && <small className="server-version">{server.installed_version}{server.installed_build ? ` · ${server.installed_build}` : ""}</small>}
                        </td>
                        <td><span className={`badge badge--${running ? "success" : server.runtime_state === "crashed" ? "danger" : "info"}`}>{t(`servers.runtime_states.${server.runtime_state}`)}</span></td>
                        <td onClick={(event) => event.stopPropagation()}><div className="server-actions">
                            {needsInstall && canInstall ? (
                                <Tooltip content={t("servers.install")} position="top"><button aria-label={t("servers.install")} className="btn btn--icon btn--ghost" disabled={busy} onClick={() => void run(server.id, "install")}><Download size={17} aria-hidden="true" /></button></Tooltip>
                            ) : running && canStartStop ? <>
                                {hasPermission("server.start") && hasPermission("server.stop") && <Tooltip content={t("servers.restart")} position="top"><button aria-label={t("servers.restart")} className="btn btn--icon btn--ghost" disabled={busy} onClick={() => void run(server.id, "restart")}><RotateCw size={17} /></button></Tooltip>}
                                {hasPermission("server.stop") && <Tooltip content={t("servers.stop")} position="top"><button aria-label={t("servers.stop")} className="btn btn--icon btn--ghost" disabled={busy} onClick={() => void run(server.id, "stop")}><Square size={17} /></button></Tooltip>}
                                {hasPermission("server.kill") && <Tooltip content={t("servers.kill")} position="top"><button aria-label={t("servers.kill")} className="btn btn--icon btn--ghost text-danger" disabled={busy} onClick={() => void run(server.id, "kill")}><Skull size={17} /></button></Tooltip>}
                            </> : !running && canStartStop && hasPermission("server.start") && server.installation_state === "installed" ? (
                                <Tooltip content={t("servers.start")} position="top"><button aria-label={t("servers.start")} className="btn btn--icon btn--ghost text-success" disabled={busy || server.installation_state !== "installed"} onClick={() => void run(server.id, "start")}><Play size={18} /></button></Tooltip>
                            ) : <span className="text-muted">—</span>}
                        </div></td>
                    </tr>
                );
            })}</tbody>
        </Table>
    );
}
