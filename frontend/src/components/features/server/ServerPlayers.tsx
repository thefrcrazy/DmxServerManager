import { useState } from "react";
import { Users, Shield, Ban, LogOut, Plus, Search, Trash2, Calendar, Database, Star, RotateCw } from "lucide-react";
import ActionMenu from "./ActionMenu";
import { formatDate } from "@/utils/formatters";
import { useLanguage } from "@/contexts/LanguageContext";

export interface Player {
    name: string;
    uuid?: string;
    is_online: boolean;
    last_seen?: string;
    player_ip?: string;
    is_op?: boolean;
    is_banned?: boolean;
    is_whitelisted?: boolean;
    reason?: string;
    bannedBy?: string;
    expires?: string;
    created?: string;
}

interface ServerPlayersProps {
    players: Player[]; // Online players
    playerList: Player[]; // From JSON files
    activeTab: "online" | "whitelist" | "bans" | "ops" | "database";
    onTabChange: (tab: "online" | "whitelist" | "bans" | "ops" | "database") => void;
    isLoading: boolean;
    onAction: (action: string, player: Player) => void;
    onAddPlayer: () => void;
    onRemovePlayer: (player: Player) => void;
    onRefresh: () => void;
}

export default function ServerPlayers({
    players,
    playerList,
    activeTab,
    onTabChange,
    isLoading,
    onAction,
    onAddPlayer,
    onRemovePlayer,
    onRefresh
}: ServerPlayersProps) {
    const { t } = useLanguage();
    const [searchTerm, setSearchTerm] = useState("");

    // Determine which list to show
    const displayList = activeTab === "online" ? players : playerList;

    // Filter
    const filteredList = displayList.filter(p =>
        (p.name || "").toLowerCase().includes(searchTerm.toLowerCase()) ||
        (p.uuid || "").toLowerCase().includes(searchTerm.toLowerCase())
    );

    return (
        <div className="card players-card">
            <div className="players-header">
                <div className="header-top">
                    <h3 className="section-title">
                        <Users size={20} />
                        {t("server_detail.players.title")}
                    </h3>
                    <div className="header-actions">
                        <button onClick={onRefresh} className="btn btn--secondary btn--sm">
                            <RotateCw size={14} className={isLoading ? "spin" : ""} />
                            {t("common.refresh")}
                        </button>
                    </div>
                </div>

                {/* Navigation Pills */}
                <div className="players-nav">
                    {[
                        { id: "online", icon: Users, label: t("server_detail.players.tabs.online") },
                        { id: "database", icon: Database, label: t("server_detail.players.tabs.database") },
                        { id: "whitelist", icon: Shield, label: t("server_detail.players.tabs.whitelist") },
                        { id: "ops", icon: Star, label: t("server_detail.players.tabs.permissions") },
                        { id: "bans", icon: Ban, label: t("server_detail.players.tabs.bans") },
                    ].map((tab) => (
                        <button
                            key={tab.id}
                            onClick={() => onTabChange(tab.id as any)}
                            className={`nav-pill ${activeTab === tab.id ? "active" : ""}`}
                        >
                            <tab.icon size={14} />
                            {tab.label}
                        </button>
                    ))}
                </div>

                {/* Toolbar */}
                <div className="players-toolbar">
                    <div className="search-wrapper">
                        <Search size={16} className="search-icon" />
                        <input
                            type="text"
                            placeholder={t("server_detail.players.search_placeholder")}
                            className="input search-input"
                            value={searchTerm}
                            onChange={e => setSearchTerm(e.target.value)}
                        />
                    </div>
                    {(activeTab !== "online" && activeTab !== "database") && (
                        <button onClick={onAddPlayer} className="btn btn--primary btn--sm">
                            <Plus size={16} /> {t("common.add")}
                        </button>
                    )}
                </div>
            </div>

            <div className="players-content">
                {isLoading ? (
                    <div className="loading-container">
                        <div className="spinner"></div>
                    </div>
                ) : filteredList.length === 0 ? (
                    <div className="empty-state">
                        <Users size={32} />
                        <span>{t("server_detail.players.no_players")}</span>
                    </div>
                ) : (
                    <div className="table-responsive">
                        <table className="players-table">
                            <thead>
                                <tr>
                                    <th className="th-player">{t("server_detail.players.headers.player")}</th>
                                    {activeTab === "bans" ? (
                                        <>
                                            <th className="th-reason">{t("server_detail.players.headers.reason_banned_by")}</th>
                                            <th className="th-date">{t("server_detail.players.headers.expiration")}</th>
                                        </>
                                    ) : (
                                        <th className="th-status">{t("server_detail.players.headers.status_info")}</th>
                                    )}
                                    <th className="th-actions">{t("server_detail.players.headers.actions")}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {filteredList.map((player, idx) => (
                                    <tr key={player.uuid || idx}>
                                        <td className="td-player">
                                            <div className="player-badge">
                                                <div className="player-avatar">
                                                    {(player.name || "?").charAt(0).toUpperCase()}
                                                </div>
                                                <div className="player-info">
                                                    <div className="player-name">
                                                        {player.name && player.name.length <= 16 ? player.name : t("common.unknown")}
                                                    </div>
                                                    <div className="player-meta-row">
                                                        {(player.uuid || (player.name && player.name.length > 16)) && (
                                                            <span className="player-uuid" title={player.uuid || player.name}>
                                                                {(player.uuid || player.name).substring(0, 8)}...
                                                            </span>
                                                        )}
                                                        {player.player_ip && <span className="player-ip">{player.player_ip}</span>}
                                                    </div>
                                                </div>
                                            </div>
                                        </td>

                                        {activeTab === "bans" ? (
                                            <>
                                                <td className="td-reason">
                                                    <div className="ban-reason">
                                                        <span className="reason-text">{player.reason || t("server_detail.messages.action_error")?.replace("action_error", "no_reason") || "No reason"}</span>
                                                        {player.bannedBy && <span className="banned-by">par {player.bannedBy}</span>}
                                                    </div>
                                                </td>
                                                <td className="td-date">
                                                    {player.expires ? (
                                                        <div className="expiration-date" title={formatDate(player.expires)}>
                                                            <Calendar size={12} />
                                                            {new Date(player.expires).toLocaleDateString()}
                                                        </div>
                                                    ) : (
                                                        <span className="badge badge--danger">Permanent</span>
                                                    )}
                                                </td>
                                            </>
                                        ) : (
                                            <td className="td-status">
                                                <div className="status-container">
                                                    {activeTab === "online" ? (
                                                        <span className="status-badge status-badge--online">{t("server_detail.players.status.connected")}</span>
                                                    ) : activeTab === "whitelist" ? (
                                                        <span className="text-muted text-sm">{t("server_detail.players.status.whitelisted")}</span>
                                                    ) : activeTab === "database" ? (
                                                        <span className="text-muted text-sm">{t("server_detail.players.status.offline")}</span>
                                                    ) : (
                                                        <span className="text-muted text-sm">{t("server_detail.players.status.group")} {player.reason || "Default"}</span>
                                                    )}
                                                    {player.last_seen && (
                                                        <div className="last-seen">
                                                            <Calendar size={10} />
                                                            {formatDate(player.last_seen)}
                                                        </div>
                                                    )}
                                                </div>
                                            </td>
                                        )}

                                        <td className="td-actions">
                                            <div className="action-buttons">
                                                {(() => {
                                                    const actions = [];
                                                    if (activeTab === "online") {
                                                        const isOp = player.is_op;
                                                        actions.push({
                                                            label: isOp ? t("server_detail.players.actions.deop") : t("server_detail.players.actions.op"),
                                                            icon: Shield,
                                                            onClick: () => onAction(isOp ? "deop" : "op", player)
                                                        });
                                                        actions.push({
                                                            label: t("server_detail.players.actions.kick"),
                                                            icon: LogOut,
                                                            onClick: () => onAction("kick", player),
                                                            variant: "warning" as const
                                                        });
                                                        actions.push({
                                                            label: t("server_detail.players.actions.ban"),
                                                            icon: Ban,
                                                            onClick: () => onAction("ban", player),
                                                            variant: "danger" as const
                                                        });
                                                    } else {
                                                        actions.push({
                                                            label: t("server_detail.players.actions.remove"),
                                                            icon: Trash2,
                                                            onClick: () => onRemovePlayer(player),
                                                            variant: "danger" as const
                                                        });
                                                    }
                                                    return <ActionMenu actions={actions} />;
                                                })()}
                                            </div>
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </table>
                    </div>
                )}
            </div>
        </div>
    );
}
