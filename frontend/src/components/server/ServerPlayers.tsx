import { useState } from "react";
import { Users, Shield, Ban, LogOut, Plus, Search, Trash2 } from "lucide-react";
import { useDialog } from "../../contexts/DialogContext";

interface Player {
    name: string;
    is_online: boolean;
    last_seen: string;
    is_op?: boolean;
    is_banned?: boolean;
    is_whitelisted?: boolean;
    level?: number;
    reason?: string;
}

interface ServerPlayersProps {
    players: Player[]; // Online players from server status
    playerList: Player[]; // From JSON files (whitelist, bans, ops)
    // listType was replaced by activeTab 
    // Actually listType was passed as activePlayerTab from parent.
    // I can just reuse listType as the activeTab prop.
    // So rename listType to activeTab for clarity or keep it.
    // Let's keep listType as the prop name for the TAB, but maybe rename it to activeTab to match component logic.
    // The component used activeTab internally.
    // Let's add onTabChange.
    activeTab: "online" | "whitelist" | "bans" | "ops";
    onTabChange: (tab: "online" | "whitelist" | "bans" | "ops") => void;
    isLoading: boolean;
    onAction: (action: string, playerName: string) => void;
    onAddPlayer: (name: string) => void;
    onRemovePlayer: (name: string) => void;
    onRefresh: () => void;
}

export default function ServerPlayers({
    players,
    playerList,
    activeTab, // Use prop
    onTabChange, // Use prop
    isLoading,
    onAction,
    onAddPlayer,
    onRemovePlayer,
    onRefresh
}: ServerPlayersProps) {
    const { prompt } = useDialog();
    const [searchTerm, setSearchTerm] = useState("");

    // Determine which list to show
    const displayList = activeTab === "online" ? players : playerList;

    // Filter
    const filteredList = displayList.filter(p =>
        p.name.toLowerCase().includes(searchTerm.toLowerCase())
    );

    const handleAdd = async () => {
        const name = await prompt("Nom du joueur :");
        if (name) onAddPlayer(name);
    };

    return (
        <div className="card players-card">
            <div className="players-header">
                <div className="header-top">
                    <h3 className="section-title">
                        <Users size={20} />
                        Gestion des Joueurs
                    </h3>
                    <button onClick={onRefresh} className="btn btn--secondary btn--sm">
                        Actualiser
                    </button>
                </div>

                {/* Navigation Pills */}
                <div className="players-nav">
                    {[
                        { id: "online", icon: Users, label: "En ligne" },
                        { id: "whitelist", icon: Shield, label: "Whitelist" },
                        { id: "ops", icon: Star, label: "Opérateurs" },
                        { id: "bans", icon: Ban, label: "Bannis" },
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
                            placeholder="Rechercher un joueur..."
                            className="input search-input"
                            value={searchTerm}
                            onChange={e => setSearchTerm(e.target.value)}
                        />
                    </div>
                    {activeTab !== "online" && (
                        <button onClick={handleAdd} className="btn btn--primary btn--sm">
                            <Plus size={16} /> Ajouter
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
                        <span>Aucun joueur trouvé</span>
                    </div>
                ) : (
                    <div className="table-responsive">
                        <table className="players-table">
                            <thead>
                                <tr>
                                    <th className="th-player">Joueur</th>
                                    <th className="th-status">Statut</th>
                                    {activeTab === "bans" && <th className="th-reason">Raison</th>}
                                    <th className="th-actions">Actions</th>
                                </tr>
                            </thead>
                            <tbody>
                                {filteredList.map((player, idx) => (
                                    <tr key={idx}>
                                        <td className="td-player">
                                            <div className="player-badge">
                                                <div className="player-avatar">
                                                    {player.name.charAt(0).toUpperCase()}
                                                </div>
                                                <div className="player-name">{player.name}</div>
                                            </div>
                                        </td>
                                        <td className="td-status">
                                            {activeTab === "online" ? (
                                                <span className="status-badge status-badge--online">
                                                    Connecté
                                                </span>
                                            ) : (
                                                <span className="status-text">
                                                    {activeTab === "whitelist" ? "Whitelisted" :
                                                        activeTab === "ops" ? "Opérateur" : "Banni"}
                                                </span>
                                            )}
                                        </td>
                                        {activeTab === "bans" && (
                                            <td className="td-reason">
                                                {player.reason || "Aucune raison"}
                                            </td>
                                        )}
                                        <td className="td-actions">
                                            <div className="action-buttons">
                                                {activeTab === "online" && (
                                                    <>
                                                        <button
                                                            onClick={() => onAction("op", player.name)}
                                                            title={player.is_op ? "Retirer OP" : "Donner OP"}
                                                            className={`btn btn--icon btn--xs ${player.is_op ? "btn--primary" : "btn--ghost"}`}
                                                        >
                                                            <Shield size={14} />
                                                        </button>
                                                        <button
                                                            onClick={() => onAction("kick", player.name)}
                                                            title="Kicker"
                                                            className="btn btn--icon btn--xs btn--ghost hover-warning"
                                                        >
                                                            <LogOut size={14} />
                                                        </button>
                                                        <button
                                                            onClick={() => onAction("ban", player.name)}
                                                            title="Bannir"
                                                            className="btn btn--icon btn--xs btn--ghost hover-danger"
                                                        >
                                                            <Ban size={14} />
                                                        </button>
                                                    </>
                                                )}
                                                {activeTab !== "online" && (
                                                    <button
                                                        onClick={() => onRemovePlayer(player.name)}
                                                        className="btn btn--icon btn--xs btn--ghost hover-danger"
                                                        title="Supprimer de la liste"
                                                    >
                                                        <Trash2 size={14} />
                                                    </button>
                                                )}
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

function Star({ size, className }: { size: number, className?: string }) {
    return <svg xmlns="http://www.w3.org/2000/svg" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"></polygon></svg>;
}
