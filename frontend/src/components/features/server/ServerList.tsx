import React from "react";
import { Link, useNavigate } from "react-router-dom";
import {
    Play, Square, RotateCw, Skull, Server as ServerIcon, AlertTriangle
} from "lucide-react";
import { useState } from "react";
import { useToast } from "@/contexts/ToastContext";
import { Server } from "@/schemas/api";
import { Table, Tooltip } from "@/components/ui";
import ServerCard from "./ServerCard";
import { useLanguage } from "@/contexts/LanguageContext";
import { formatBytes, formatGB } from "@/utils/formatters";
import { getGameLogo } from "@/utils/gameConfig";

interface ServerListProps {
    servers: Server[];
    viewMode: "grid" | "list";
    onAction: (id: string, action: "start" | "stop" | "restart" | "kill") => Promise<boolean | void>;
}

export default function ServerList({ servers, viewMode, onAction }: ServerListProps) {
    const { t } = useLanguage();
    const navigate = useNavigate();
    const { success, error } = useToast();
    const [loadingAction, setLoadingAction] = useState<string | null>(null);

    if (viewMode === "grid") {
        return (
            <div className="server-grid" style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(300px, 1fr))",
                gap: "1.5rem"
            }}>
                {servers.map(server => (
                    <ServerCard key={server.id} server={server} onAction={onAction} />
                ))}
            </div>
        );
    }

    const handleRowClick = (e: React.MouseEvent, serverId: string) => {
        if ((e.target as HTMLElement).closest("button") || (e.target as HTMLElement).closest("a.btn")) {
            return;
        }
        navigate(`/servers/${serverId}`);
    };

    const handleActionClick = async (e: React.MouseEvent, serverId: string, action: "start" | "stop" | "restart" | "kill") => {
        e.stopPropagation();
        if (loadingAction) return;

        setLoadingAction(`${serverId}-${action}`);
        try {
            const result = await onAction(serverId, action);
            if (result) {
                success(t(`servers.action_${action}_success`) || `Action ${action} successful`);
            } else {
                // onAction should ideally return the error or throw it
                error(t("servers.action_failed") || "Action failed");
            }
        } catch (err: any) {
            const msg = err instanceof Error ? err.message : String(err);
            error(t(msg) || t("servers.action_failed") || "Action failed");
        } finally {
            setLoadingAction(null);
        }
    };

    return (
        <Table>
            <thead>
                <tr>
                    <th className="col-server">{t("servers.server_header")}</th>
                    <th className="col-actions">{t("servers.actions")}</th>
                    <th className="col-cpu">{t("dashboard.cpu_usage")}</th>
                    <th className="col-mem">{t("dashboard.ram_usage")}</th>
                    <th className="col-disk">{t("dashboard.disk_usage")}</th>
                    <th className="col-players">{t("servers.players")}</th>
                    <th className="col-status">{t("servers.status")}</th>
                </tr>
            </thead>
            <tbody>
                {servers.map(server => {
                    const isRunning = server.status === "running";
                    const isMissing = server.status === "missing";
                    const isInstalling = server.status === "installing";
                    const isAuthRequired = server.status === "auth_required";
                    const isActionLoading = loadingAction?.startsWith(`${server.id}-`);

                    return (
                        <tr
                            key={server.id}
                            className={`server-row ${isRunning ? "server-row--running" : ""} ${isMissing ? "server-row--missing" : ""} ${isInstalling ? "server-row--installing" : ""} ${isActionLoading ? "server-row--disabled" : ""}`}
                            onClick={(e) => handleRowClick(e, server.id)}
                            style={{ cursor: isActionLoading ? "default" : "pointer" }}
                        >
                            <td>
                                <div className="server-name">
                                    <div className={`server-icon ${isRunning ? "server-icon--running" : ""} ${isMissing ? "server-icon--missing" : ""} ${isInstalling ? "server-icon--installing" : ""}`}
                                        style={{ background: "transparent", borderRadius: 0, padding: 0 }}>
                                        {getGameLogo(server.game_type) ? (
                                            <img src={getGameLogo(server.game_type)} alt={server.game_type} width="18" />
                                        ) : isMissing ? (
                                            <AlertTriangle size={18} />
                                        ) : isAuthRequired ? (
                                            <AlertTriangle size={18} className="text-warning" />
                                        ) : isInstalling ? (
                                            <RotateCw size={18} className="spin" />
                                        ) : (
                                            <ServerIcon size={18} />
                                        )}
                                    </div>
                                    <div className="server-link text-inherit">
                                        {server.name}
                                    </div>
                                </div>
                            </td>
                            <td>
                                <div className="server-actions">
                                    {isMissing ? (
                                        <span className="server-actions__corrupt">
                                            <AlertTriangle size={14} />
                                            {t("servers.corrupt")}
                                        </span>
                                    ) : isInstalling ? (
                                        <span className="text-info text-sm flex items-center gap-1">
                                            <RotateCw size={14} className="spin" /> {t("servers.installing")}
                                        </span>
                                    ) : isAuthRequired ? (
                                        <Link to={`/servers/${server.id}`} className="btn btn--sm btn--warning">
                                            {t("servers.authenticate")}
                                        </Link>
                                    ) : isRunning ? (
                                        <>
                                            <Tooltip content={t("servers.restart")} position="top">
                                                <button
                                                    className="btn btn--icon btn--ghost text-info"
                                                    onClick={(e) => handleActionClick(e, server.id, "restart")}
                                                    disabled={!!loadingAction}
                                                >
                                                    {loadingAction === `${server.id}-restart` ? <RotateCw size={16} className="spin" /> : <RotateCw size={16} />}
                                                </button>
                                            </Tooltip>
                                            <Tooltip content={t("servers.stop")} position="top">
                                                <button
                                                    className="btn btn--icon btn--ghost text-danger"
                                                    onClick={(e) => handleActionClick(e, server.id, "stop")}
                                                    disabled={!!loadingAction}
                                                >
                                                    {loadingAction === `${server.id}-stop` ? <RotateCw size={16} className="spin" /> : <Square size={16} />}
                                                </button>
                                            </Tooltip>
                                            <Tooltip content={t("servers.kill")} position="top">
                                                <button
                                                    onClick={(e) => handleActionClick(e, server.id, "kill")}
                                                    className="btn btn--icon btn--ghost text-danger btn-kill"
                                                    disabled={!!loadingAction}
                                                >
                                                    {loadingAction === `${server.id}-kill` ? <RotateCw size={16} className="spin" /> : <Skull size={16} />}
                                                </button>
                                            </Tooltip>
                                        </>
                                    ) : (
                                        <Tooltip content={t("servers.start")} position="top">
                                            <button
                                                className="btn btn--icon btn--ghost text-success"
                                                onClick={(e) => handleActionClick(e, server.id, "start")}
                                                disabled={!!loadingAction}
                                            >
                                                {loadingAction === `${server.id}-start` ? <RotateCw size={18} className="spin" /> : <Play size={18} />}
                                            </button>
                                        </Tooltip>
                                    )}
                                </div>
                            </td>
                            <td>
                                <div className="usage-bar">
                                    <div className="usage-bar__track">
                                        <div
                                            className="usage-bar__fill usage-bar__fill--cpu"
                                            style={{ width: `${Math.min(100, server.cpu_usage_normalized || 0)}%` }}
                                        />
                                    </div>
                                    <span className="usage-bar__text">{(server.cpu_usage_normalized || 0).toFixed(1)}%</span>
                                </div>
                            </td>
                            <td title={`Heap: ${formatBytes(server.max_heap_bytes)} + Java: ${formatBytes(server.max_memory_bytes - server.max_heap_bytes)}`}>
                                <div className="usage-bar">
                                    <div className="usage-bar__track">
                                        <div
                                            className={`usage-bar__fill ${server.memory_usage_bytes > server.max_memory_bytes ? "usage-bar__fill--danger" : "usage-bar__fill--mem"}`}
                                            style={{ width: `${Math.min(100, (server.memory_usage_bytes / (server.max_memory_bytes || 1)) * 100)}%` }}
                                        />
                                    </div>
                                    <span className="usage-bar__text">
                                        {formatGB(server.memory_usage_bytes)} / {formatGB(server.max_memory_bytes)}
                                    </span>
                                </div>
                            </td>
                            <td className="text-cell">
                                {server.disk_usage_bytes > 0 ? formatBytes(server.disk_usage_bytes) : "--"}
                            </td>
                            <td className="text-cell">
                                {server.players ? `${server.players.length}` : "0"} / {server.max_players || "?"} Max
                            </td>
                            <td className="text-right">
                                <span className={`badge badge--${isMissing ? "warning" : isAuthRequired ? "warning" : isInstalling ? "info" : server.status === "running" ? "success" : "danger"}`}>
                                    {isMissing ? t("servers.missing") : isAuthRequired ? t("servers.auth_required") : isInstalling ? t("servers.installing").replace("...", "") : server.status === "running" ? t("servers.online") : t("servers.offline")}
                                </span>
                            </td>
                        </tr>
                    );
                })}
            </tbody>
        </Table>
    );
}
