import { AlertTriangle, Download, Play, RotateCw, Skull, Square } from "lucide-react";
import { useNavigate } from "react-router-dom";
import type { ConnectionInfo, Instance } from "@/schemas/api";
import type { ServerAction } from "@/services/api/server.client";
import { Button, Card, Tooltip } from "@/components/ui";
import { fallbackGameArtwork, gameProfileVisual } from "@/constants/gameProfiles";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePermission } from "@/hooks";
import MaskedConnection from "./MaskedConnection";

interface ServerCardProps {
    server: Instance;
    capabilities: ReadonlySet<string>;
    playerCount?: number;
    connection?: ConnectionInfo;
    onAction: (id: string, action: ServerAction) => void;
}

function stateLabel(server: Instance, t: (key: string) => string): string {
    if (server.installation_state !== "installed") return t(`servers.installation_states.${server.installation_state}`);
    return t(`servers.runtime_states.${server.runtime_state}`);
}

export default function ServerCard({ server, capabilities, playerCount, connection, onAction }: ServerCardProps) {
    const { t } = useLanguage();
    const { hasPermission } = usePermission();
    const navigate = useNavigate();
    const running = server.runtime_state === "running";
    const transitioning = ["starting", "stopping"].includes(server.runtime_state)
        || ["installing", "updating"].includes(server.installation_state);
    const needsInstall = ["not_installed", "failed"].includes(server.installation_state);
    const installed = server.installation_state === "installed";
    const canInstall = capabilities.has("install") && hasPermission("server.update_game");
    const canStartStop = capabilities.has("lifecycle");
    const visual = gameProfileVisual(server.profile_id);

    const handleCardClick = (event: React.MouseEvent) => {
        if (!(event.target as HTMLElement).closest("button")) navigate(`/servers/${server.id}`);
    };
    const action = (event: React.MouseEvent, value: ServerAction) => {
        event.stopPropagation();
        onAction(server.id, value);
    };

    return (
        <Card
            className={`server-card ${running ? "server-card--running" : ""}`}
            onClick={handleCardClick}
            role="link"
            tabIndex={0}
            aria-label={`${t("servers.open_server")} ${server.name}`}
            onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    navigate(`/servers/${server.id}`);
                }
            }}
        >
            <div className="server-card__artwork">
                <img
                    src={visual.artwork}
                    alt=""
                    loading="lazy"
                    referrerPolicy="no-referrer"
                    style={{ objectPosition: visual.artworkPosition }}
                    onError={(event) => fallbackGameArtwork(event, visual.fallbackArtwork)}
                />
                <span className={`server-card__state badge badge--${running ? "success" : needsInstall ? "warning" : "info"}`}>
                    {transitioning && <RotateCw size={13} className="spin" aria-hidden="true" />}
                    {(server.runtime_state === "crashed" || server.installation_state === "failed")
                        && <AlertTriangle size={13} aria-hidden="true" />}
                    {stateLabel(server, t)}
                </span>
            </div>
            <div className="server-card__body">
                <div className="server-card__header">
                    <div>
                        <span className="server-card__profile">{visual.label}</span>
                        <h3 className="server-card__title">{server.name}</h3>
                    </div>
                    <span className={`server-card__live-dot ${running ? "server-card__live-dot--running" : ""}`} aria-hidden="true" />
                </div>

                <div className="server-card__stats">
                    <div className="server-card__stat-row"><span>{t("servers.status")}</span><span>{stateLabel(server, t)}</span></div>
                    <div className="server-card__stat-row"><span>{t("servers.players")}</span><span>{playerCount ?? "—"}</span></div>
                    <div className="server-card__stat-row"><span>{t("servers.installed_version")}</span><span>{server.installed_version ?? server.installed_build ?? "—"}</span></div>
                    <div className="server-card__stat-row server-card__stat-row--connection"><span>{t("servers.connection")}</span><MaskedConnection connection={connection} compact /></div>
                </div>

                <div className="server-card__actions">
                    {needsInstall && canInstall ? (
                        <Button variant="success" size="sm" fullWidth onClick={(event) => action(event, "install")}>
                            <Download size={16} aria-hidden="true" />{t("servers.install")}
                        </Button>
                    ) : running && canStartStop ? (
                        <>
                            {hasPermission("server.start") && hasPermission("server.stop") && <Tooltip content={t("servers.restart")} position="top">
                                <Button aria-label={t("servers.restart")} variant="ghost" size="icon" onClick={(event) => action(event, "restart")}><RotateCw size={18} /></Button>
                            </Tooltip>}
                            {hasPermission("server.stop") && <Tooltip content={t("servers.stop")} position="top">
                                <Button aria-label={t("servers.stop")} variant="ghost" size="icon" onClick={(event) => action(event, "stop")}><Square size={18} /></Button>
                            </Tooltip>}
                            {hasPermission("server.kill") && <Tooltip content={t("servers.kill")} position="top">
                                <Button aria-label={t("servers.kill")} variant="ghost" size="icon" onClick={(event) => action(event, "kill")}><Skull size={18} /></Button>
                            </Tooltip>}
                        </>
                    ) : !running && canStartStop && installed && hasPermission("server.start") ? (
                        <Button variant="success" size="sm" fullWidth disabled={transitioning} onClick={(event) => action(event, "start")}>
                            <Play size={16} />{t("servers.start")}
                        </Button>
                    ) : null}
                </div>
            </div>
        </Card>
    );
}
