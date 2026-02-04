import React, { useState, useMemo } from "react";
import { Search, User, Plus, X } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";

interface Player {
    name: string;
    uuid?: string;
    is_online: boolean;
    last_seen?: string;
}

interface AddPlayerModalProps {
    isOpen: boolean;
    onClose: () => void;
    onAdd: (name: string) => void;
    knownPlayers: Player[];
    title?: string;
}

export default function AddPlayerModal({ isOpen, onClose, onAdd, knownPlayers, title }: AddPlayerModalProps) {
    const { t } = useLanguage();
    const [search, setSearch] = useState("");

    const filtered = useMemo(() => {
        if (!search) return knownPlayers.slice(0, 5); // Show top 5 recent/known
        return knownPlayers.filter(p =>
            p.name.toLowerCase().includes(search.toLowerCase()) ||
            (p.uuid && p.uuid.toLowerCase().includes(search.toLowerCase()))
        ).slice(0, 10);
    }, [knownPlayers, search]);

    if (!isOpen) return null;

    const handleSubmit = (e: React.FormEvent) => {
        e.preventDefault();
        if (search.trim()) {
            onAdd(search.trim());
            setSearch("");
        }
    };

    return (
        <div className="modal-overlay" onClick={onClose}>
            <div className="modal" onClick={e => e.stopPropagation()}>
                <div className="modal__header">
                    <h3>{title || t("server_detail.players.add_prompt")}</h3>
                    <button className="modal-close" onClick={onClose} style={{ background: "none", border: "none", color: "var(--text-secondary)", cursor: "pointer" }}><X size={20} /></button>
                </div>
                <div className="modal__body">
                    <form onSubmit={handleSubmit} className="player-search-form">
                        <div className="input-wrapper">
                            <div className="input-wrapper__icon">
                                <Search size={16} />
                            </div>
                            <input
                                type="text"
                                value={search}
                                onChange={e => setSearch(e.target.value)}
                                placeholder={t("server_detail.players.search_placeholder")}
                                autoFocus
                                className="input input--with-icon"
                            />
                        </div>
                    </form>

                    <div className="known-players-list">
                        <div className="list-label">{t("server_detail.players.suggestions")} {knownPlayers.length > 0 ? "" : t("server_detail.players.no_known_players")}</div>
                        {filtered.map(p => (
                            <div key={p.uuid || p.name} className="player-suggestion-item" onClick={() => onAdd(p.name)}>
                                <div className="player-avatar">
                                    <User size={16} />
                                </div>
                                <div className="player-info">
                                    <div className="player-name">{p.name}</div>
                                    {p.uuid && <div className="player-uuid">{p.uuid}</div>}
                                </div>
                                <button className="btn-icon-small"><Plus size={14} /></button>
                            </div>
                        ))}
                        {search && !filtered.find(p => p.name.toLowerCase() === search.toLowerCase()) && (
                            <div className="player-suggestion-item new-entry" onClick={() => onAdd(search)}>
                                <div className="player-avatar"><Plus size={16} /></div>
                                <div className="player-info">
                                    <div className="player-name">{t("common.add")} "{search}"</div>
                                    <div className="player-uuid">{t("server_detail.players.new_player")}</div>
                                </div>
                            </div>
                        )}
                    </div>
                </div>
                <div className="modal__footer">
                    <button className="btn btn--secondary" onClick={onClose}>{t("common.cancel")}</button>
                    <button className="btn btn--primary" onClick={() => onAdd(search)} disabled={!search}>{t("common.add")}</button>
                </div>
            </div>

            <style>{`
                .modal__header {
                    display: flex; justify-content: space-between; align-items: center;
                }
                .modal__header h3 { margin: 0; font-size: 1.1rem; }
                
                .player-search-form { margin-bottom: 16px; }
                
                /* List Styles */
                .known-players-list {
                    max-height: 300px; overflow-y: auto;
                    display: flex; flex-direction: column; gap: 8px;
                }
                .list-label { font-size: 0.8rem; color: var(--text-muted); margin-bottom: 4px; }
                
                .player-suggestion-item {
                    display: flex; align-items: center; gap: 10px;
                    padding: 8px;
                    background: var(--bg-background);
                    border: 1px solid var(--border-card);
                    border-radius: 6px;
                    cursor: pointer;
                    transition: background 0.2s;
                }
                .player-suggestion-item:hover { background: var(--bg-hover); }
                .player-avatar {
                    width: 32px; height: 32px;
                    background: var(--bg-secondary);
                    border-radius: 50%;
                    display: flex; align-items: center; justify-content: center;
                    color: var(--text-secondary);
                }
                .player-info { flex: 1; min-width: 0; }
                .player-name { font-weight: 500; }
                .player-uuid { font-size: 0.75rem; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
                .btn-icon-small { background: none; border: none; color: var(--primary-color); }
                
                .new-entry { border-style: dashed; border-color: var(--primary-color); }
            `}</style>
        </div>
    );
}
