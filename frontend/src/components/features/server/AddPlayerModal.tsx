import React, { useState, useMemo } from "react";
import { Search, User, Plus } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Modal, Button } from "@/components/ui";

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

    const footer = (
        <>
            <Button variant="secondary" onClick={onClose}>{t("common.cancel")}</Button>
            <Button onClick={() => onAdd(search)} disabled={!search}>{t("common.add")}</Button>
        </>
    );

    return (
        <Modal
            isOpen={isOpen}
            onClose={onClose}
            title={title || t("server_detail.players.add_prompt")}
            footer={footer}
        >
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

            <style>{`
                .player-search-form { margin-bottom: 16px; }
                
                /* List Styles */
                .known-players-list {
                    max-height: 300px; overflow-y: auto;
                    display: flex; flex-direction: column; gap: 8px;
                }
                .list-label { font-size: 0.8rem; color: var(--color-text-muted); margin-bottom: 4px; }
                
                .player-suggestion-item {
                    display: flex; align-items: center; gap: 10px;
                    padding: 8px;
                    background: var(--color-bg-primary);
                    border: 1px solid var(--color-border);
                    border-radius: 6px;
                    cursor: pointer;
                    transition: background 0.2s;
                }
                .player-suggestion-item:hover { background: var(--color-bg-elevated); }
                .player-avatar {
                    width: 32px; height: 32px;
                    background: var(--color-bg-secondary);
                    border-radius: 50%;
                    display: flex; align-items: center; justify-content: center;
                    color: var(--color-text-secondary);
                }
                .player-info { flex: 1; min-width: 0; }
                .player-name { font-weight: 500; }
                .player-uuid { font-size: 0.75rem; color: var(--color-text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
                .btn-icon-small { background: none; border: none; color: var(--color-accent); }
                
                .new-entry { border-style: dashed; border-color: var(--color-accent); }
            `}</style>
        </Modal>
    );
}