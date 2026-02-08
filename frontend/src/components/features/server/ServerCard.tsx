import React from "react";
import { Link, useNavigate } from "react-router-dom";
import {
    Play, Square, RotateCw, Skull, Server as ServerIcon, AlertTriangle, Users
} from "lucide-react";
import { Server } from "@/schemas/api";
import { formatGB } from "@/utils/formatters";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip, Card, Button } from "@/components/ui";
import { getGameLogo } from "@/utils/gameConfig";

interface ServerCardProps {
    server: Server;
    onAction: (id: string, action: "start" | "stop" | "restart" | "kill") => void;
}

export default function ServerCard({ server, onAction }: ServerCardProps) {
    const { t } = useLanguage();
    const navigate = useNavigate();

    const isRunning = server.status === "running";
    const isMissing = server.status === "missing";
    const isInstalling = server.status === "installing";
    const isAuthRequired = server.status === "auth_required";

    const handleCardClick = (e: React.MouseEvent) => {
        // Prevent navigation if clicking on buttons
        if ((e.target as HTMLElement).closest("button") || (e.target as HTMLElement).closest("a.btn")) {
            return;
        }
        navigate(`/servers/${server.id}`);
    };

    const handleActionClick = (e: React.MouseEvent, action: "start" | "stop" | "restart" | "kill") => {
        e.stopPropagation();
        onAction(server.id, action);
    };

    return (
        <Card className={`server-card ${isRunning ? "server-card--running" : ""} ${isMissing ? "server-card--missing" : ""}`} onClick={handleCardClick}>
            <div className="server-card__header">
                <div className={`server-card__icon ${isRunning ? "server-card__icon--running" : ""} ${isMissing ? "server-card__icon--missing" : ""} ${isInstalling ? "server-card__icon--installing" : ""}`}>
                    {getGameLogo(server.game_type) ? (
                        <img src={getGameLogo(server.game_type)} alt={server.game_type} />
                    ) : isMissing ? (
                        <AlertTriangle size={20} />
                    ) : isAuthRequired ? (
                        <AlertTriangle size={20} className="text-warning" />
                    ) : isInstalling ? (
                        <RotateCw size={20} className="spin" />
                    ) : (
                        <ServerIcon size={20} />
                    )}
                </div>
                <div>
                    <h3 className="server-card__title">{server.name}</h3>
                    <div className="server-card__meta">
                        <span>{server.game_type.charAt(0).toUpperCase() + server.game_type.slice(1)}</span>
                        <span>•</span>
                        <span className={`badge badge--${isMissing ? "warning" : isAuthRequired ? "warning" : isInstalling ? "info" : isRunning ? "success" : "danger"}`}>
                            {isMissing ? t("servers.missing") :
                                isAuthRequired ? t("servers.auth_required") :
                                    isInstalling ? t("servers.installing").replace("...", "") :
                                        isRunning ? t("servers.online") : t("servers.offline")}
                        </span>
                    </div>
                </div>
            </div>

            <div className="server-card__stats">
                <div className="server-card__stat-row">
                    <span className="server-card__stat-label">
                        <Users size={14} /> {t("servers.players")}
                    </span>
                    <span>{server.players?.length || 0} / {server.max_players || "?"}</span>
                </div>

                <div className="usage-bar-container">
                    <div className="server-card__progress">
                        <span className="text-muted">CPU</span>
                        <span>{server.cpu_usage.toFixed(1)}%</span>
                    </div>
                    <div className="server-card__track">
                        <div className="server-card__fill server-card__fill--cpu" style={{ width: `${Math.min(100, server.cpu_usage)}%` }} />
                    </div>
                </div>

                <div className="usage-bar-container">
                    <div className="server-card__progress">
                        <span className="text-muted">RAM</span>
                        <span>{formatGB(server.memory_usage_bytes)} / {formatGB(server.max_memory_bytes)}</span>
                    </div>
                    <div className="server-card__track">
                        <div
                            className={`server-card__fill ${server.memory_usage_bytes > server.max_memory_bytes ? "server-card__fill--danger" : "server-card__fill--mem"}`}
                            style={{ width: `${Math.min(100, (server.memory_usage_bytes / (server.max_memory_bytes || 1)) * 100)}%` }}
                        />
                    </div>
                </div>
            </div>

            <div className="server-card__actions">
                {isMissing ? (
                    <div className="text-danger flex items-center gap-2 text-sm">
                        <AlertTriangle size={16} /> {t("servers.corrupt")}
                    </div>
                ) : isInstalling ? (
                    <div className="text-info flex items-center gap-2 text-sm">
                        <RotateCw size={16} className="spin" /> {t("servers.installing")}
                    </div>
                ) : isAuthRequired ? (
                    <Link to={`/servers/${server.id}`} className="btn btn--sm btn--warning w-full justify-center">
                        {t("servers.authenticate")}
                    </Link>
                ) : isRunning ? (
                    <>
                        <Tooltip content={t("servers.restart")} position="top">
                            <Button variant="ghost" size="icon" className="text-info" onClick={(e) => handleActionClick(e, "restart")}>
                                <RotateCw size={18} />
                            </Button>
                        </Tooltip>
                        <Tooltip content={t("servers.stop")} position="top">
                            <Button variant="ghost" size="icon" className="text-danger" onClick={(e) => handleActionClick(e, "stop")}>
                                <Square size={18} />
                            </Button>
                        </Tooltip>
                        <Tooltip content={t("servers.kill")} position="top">
                            <Button variant="ghost" size="icon" className="text-danger" onClick={(e) => handleActionClick(e, "kill")}>
                                <Skull size={18} />
                            </Button>
                        </Tooltip>
                    </>
                ) : (
                    <Button variant="success" size="sm" fullWidth onClick={(e) => handleActionClick(e, "start")}>
                        <Play size={16} />
                        {t("servers.start")}
                    </Button>
                )}
            </div>
        </Card>
    );
}