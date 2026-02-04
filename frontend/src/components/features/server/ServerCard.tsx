import React from "react";
import { Link, useNavigate } from "react-router-dom";
import {
    Play, Square, RotateCw, Skull, Server as ServerIcon, AlertTriangle, Users
} from "lucide-react";
import { Server } from "@/schemas/api";
import { formatGB } from "@/utils/formatters";
import { useLanguage } from "@/contexts/LanguageContext";
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
        <div
            className={`card server-card ${isRunning ? "server-card--running" : ""} ${isMissing ? "server-card--missing" : ""}`}
            onClick={handleCardClick}
            style={{ cursor: "pointer", position: "relative", overflow: "hidden" }}
        >
            <div className="card__body" style={{ padding: "1.5rem" }}>
                <div style={{ display: "flex", alignItems: "center", gap: "1rem", marginBottom: "1rem" }}>
                    <div className={`server-icon ${isRunning ? "server-icon--running" : ""} ${isMissing ? "server-icon--missing" : ""} ${isInstalling ? "server-icon--installing" : ""}`}
                        style={{ width: "40px", height: "40px", minWidth: "40px", display: "flex", alignItems: "center", justifyContent: "center", overflow: "hidden", padding: 0 }}>
                        {getGameLogo(server.game_type) ? (
                            <img src={getGameLogo(server.game_type)} alt={server.game_type} style={{ width: "100%", height: "100%", objectFit: "contain" }} />
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
                        <h3 style={{ margin: 0, fontSize: "1.1rem", fontWeight: 600 }}>{server.name}</h3>
                        <div style={{ display: "flex", alignItems: "center", gap: "0.5rem", fontSize: "0.85rem", color: "var(--text-muted)" }}>
                            <span>{server.game_type.charAt(0).toUpperCase() + server.game_type.slice(1)}</span>
                            <span>•</span>
                            <span className={`badge badge--${isMissing ? "warning" : isAuthRequired ? "warning" : isInstalling ? "info" : isRunning ? "success" : "danger"}`}
                                style={{ padding: "0.25rem 0.5rem", borderRadius: "4px", fontSize: "0.75rem", fontWeight: 600, display: "inline-flex", alignItems: "center", lineHeight: 1 }}>
                                {isMissing ? t("servers.missing") :
                                    isAuthRequired ? t("servers.auth_required") :
                                        isInstalling ? t("servers.installing").replace("...", "") :
                                            isRunning ? t("servers.online") : t("servers.offline")}
                            </span>
                        </div>
                    </div>
                </div>

                <div className="server-stats" style={{ display: "grid", gap: "0.75rem", marginBottom: "1.5rem" }}>
                    <div className="stat-row" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: "0.9rem" }}>
                        <span style={{ display: "flex", alignItems: "center", gap: "0.5rem", color: "var(--text-muted)" }}>
                            <Users size={14} /> {t("servers.players")}
                        </span>
                        <span>{server.players?.length || 0} / {server.max_players || "?"}</span>
                    </div>

                    <div className="usage-bar-container">
                        <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.8rem", marginBottom: "0.25rem" }}>
                            <span className="text-muted">CPU</span>
                            <span>{server.cpu_usage.toFixed(1)}%</span>
                        </div>
                        <div className="usage-bar__track" style={{ background: "rgba(255, 255, 255, 0.1)", borderRadius: "4px", overflow: "hidden", height: "6px" }}>
                            <div className="usage-bar__fill usage-bar__fill--cpu" style={{ width: `${Math.min(100, server.cpu_usage)}%`, height: "100%", transition: "width 0.3s ease" }} />
                        </div>
                    </div>

                    <div className="usage-bar-container">
                        <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.8rem", marginBottom: "0.25rem" }}>
                            <span className="text-muted">RAM</span>
                            <span>{formatGB(server.memory_usage_bytes)} / {formatGB(server.max_memory_bytes)}</span>
                        </div>
                        <div className="usage-bar__track" style={{ background: "rgba(255, 255, 255, 0.1)", borderRadius: "4px", overflow: "hidden", height: "6px" }}>
                            <div
                                className={`usage-bar__fill ${server.memory_usage_bytes > server.max_memory_bytes ? "usage-bar__fill--danger" : "usage-bar__fill--mem"}`}
                                style={{ width: `${Math.min(100, (server.memory_usage_bytes / (server.max_memory_bytes || 1)) * 100)}%`, height: "100%", transition: "width 0.3s ease" }}
                            />
                        </div>
                    </div>
                </div>

                <div className="server-actions" style={{ display: "flex", gap: "0.5rem", justifyContent: "flex-end", borderTop: "1px solid rgba(255,255,255,0.05)", paddingTop: "1rem" }}>
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
                            <button
                                className="btn btn--icon btn--ghost text-info"
                                onClick={(e) => handleActionClick(e, "restart")}
                                title={t("servers.restart")}
                            >
                                <RotateCw size={18} />
                            </button>
                            <button
                                className="btn btn--icon btn--ghost text-danger"
                                onClick={(e) => handleActionClick(e, "stop")}
                                title={t("servers.stop")}
                            >
                                <Square size={18} />
                            </button>
                            <button
                                onClick={(e) => handleActionClick(e, "kill")}
                                title={t("servers.kill")}
                                className="btn btn--icon btn--ghost text-danger"
                            >
                                <Skull size={18} />
                            </button>
                        </>
                    ) : (
                        <button
                            className="btn btn--sm btn--success"
                            onClick={(e) => handleActionClick(e, "start")}
                            style={{ width: "100%", justifyContent: "center" }}
                        >
                            <Play size={16} style={{ marginRight: "0.5rem" }} />
                            {t("servers.start")}
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}
