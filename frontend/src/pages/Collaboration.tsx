import { useState, useEffect, useRef } from "react";
import { MessageSquare, StickyNote, Send, User as UserIcon } from "lucide-react";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { Tooltip } from "@/components/ui";
import Ansi from "ansi-to-react";

interface Message {
    id: string;
    user_id: string;
    username: string;
    content: string;
    type_name: string;
    created_at: string;
    accent_color?: string;
}

export default function Collaboration() {
    const { user } = useAuth();
    const { t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const [activeTab, setActiveTab] = useState<"chat" | "notes">("chat");
    const [messages, setMessages] = useState<Message[]>([]);
    const [newMessage, setNewMessage] = useState("");
    const scrollRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        setPageTitle("Collaboration Hub", "Espace d'échange pour l'équipe");
    }, [setPageTitle]);

    const fetchMessages = async () => {
        try {
            const response = await fetch("/api/v1/collaboration/messages", {
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });
            if (response.ok) {
                setMessages(await response.json());
            }
        } catch (error) {
            console.error("Failed to fetch messages", error);
        }
    };

    useEffect(() => {
        fetchMessages();
        const interval = setInterval(fetchMessages, 5000);
        return () => clearInterval(interval);
    }, []);

    useEffect(() => {
        if (scrollRef.current) {
            scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
    }, [messages, activeTab]);

    const handleSend = async (e?: React.FormEvent) => {
        e?.preventDefault();
        if (!newMessage.trim()) return;

        try {
            const response = await fetch("/api/v1/collaboration/messages", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify({
                    content: newMessage,
                    msg_type: activeTab
                }),
            });

            if (response.ok) {
                setNewMessage("");
                fetchMessages();
            }
        } catch (error) {
            console.error("Failed to send message", error);
        }
    };

    const filteredMessages = messages.filter(m => m.type_name === activeTab);

    return (
        <div className="collab-page">
            <div className="card collab-container">
                <div className="collab-sidebar">
                    <div className="collab-menu">
                        <button 
                            className={`collab-menu-item ${activeTab === "chat" ? "active" : ""}`}
                            onClick={() => setActiveTab("chat")}
                        >
                            <MessageSquare size={18} />
                            <span>Chat d'équipe</span>
                        </button>
                        <button 
                            className={`collab-menu-item ${activeTab === "notes" ? "active" : ""}`}
                            onClick={() => setActiveTab("notes")}
                        >
                            <StickyNote size={18} />
                            <span>Notes partagées</span>
                        </button>
                    </div>
                </div>

                <div className="collab-main">
                    <div className="collab-header">
                        <h3>{activeTab === "chat" ? "Discussion générale" : "Notes et Idées"}</h3>
                    </div>

                    <div className="collab-content" ref={scrollRef}>
                        {filteredMessages.length === 0 ? (
                            <div className="collab-empty">
                                {activeTab === "chat" ? <MessageSquare size={48} /> : <StickyNote size={48} />}
                                <p>Aucun message pour le moment.</p>
                            </div>
                        ) : (
                            filteredMessages.map((msg) => (
                                <div key={msg.id} className={`message-item ${msg.user_id === user?.id ? "message-item--self" : ""}`}>
                                    <div className="message-item__header">
                                        <span className="message-item__user" style={{ color: msg.accent_color }}>{msg.username}</span>
                                        <span className="message-item__time">
                                            {new Date(msg.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                                        </span>
                                    </div>
                                    <div className="message-item__content">
                                        <Ansi>{msg.content}</Ansi>
                                    </div>
                                </div>
                            ))
                        )}
                    </div>

                    <form className="collab-input-area" onSubmit={handleSend}>
                        <div className="input-wrapper">
                            <textarea 
                                className="collab-input"
                                placeholder={activeTab === "chat" ? "Écrire un message..." : "Ajouter une note..."}
                                value={newMessage}
                                onChange={(e) => setNewMessage(e.target.value)}
                                onKeyDown={(e) => {
                                    if (e.key === "Enter" && !e.shiftKey) {
                                        e.preventDefault();
                                        handleSend();
                                    }
                                }}
                            />
                            <Tooltip content={t("common.send")} position="top">
                                <button className="btn btn--primary btn--icon collab-send" type="submit" disabled={!newMessage.trim()}>
                                    <Send size={18} />
                                </button>
                            </Tooltip>
                        </div>
                        <p className="input-hint">Shift + Enter pour sauter une ligne</p>
                    </form>
                </div>
            </div>

            <style>{`
                .collab-page { height: calc(100vh - 140px); }
                .collab-container { display: flex; height: 100%; padding: 0; overflow: hidden; }
                
                .collab-sidebar { width: 250px; border-right: 1px solid var(--border-color); background: var(--bg-secondary); padding: 1rem; }
                .collab-menu { display: flex; flex-direction: column; gap: 0.5rem; }
                .collab-menu-item {
                    display: flex; align-items: center; gap: 12px; padding: 10px 12px;
                    border: none; background: none; border-radius: 8px;
                    color: var(--text-secondary); font-size: 0.95rem; font-weight: 500;
                    cursor: pointer; transition: 0.2s; text-align: left;
                }
                .collab-menu-item:hover { background: var(--bg-hover); color: var(--text-primary); }
                .collab-menu-item.active { background: var(--bg-tertiary); color: var(--primary-color); }
                
                .collab-main { flex: 1; display: flex; flex-direction: column; background: var(--bg-primary); }
                .collab-header { padding: 1rem 1.5rem; border-bottom: 1px solid var(--border-color); background: var(--bg-secondary); }
                .collab-header h3 { margin: 0; font-size: 1.1rem; }
                
                .collab-content { flex: 1; overflow-y: auto; padding: 1.5rem; display: flex; flex-direction: column; gap: 1rem; }
                .collab-empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; color: var(--text-muted); opacity: 0.5; gap: 1rem; }
                
                .message-item { display: flex; flex-direction: column; gap: 6px; max-width: 70%; }
                .message-item--self { align-self: flex-end; align-items: flex-end; }
                .message-item__header { display: flex; align-items: center; gap: 8px; font-size: 0.8rem; }
                .message-item__user { font-weight: 600; }
                .message-item__time { color: var(--text-muted); font-size: 0.75rem; }
                .message-item__content { 
                    padding: 12px 16px; background: var(--bg-tertiary); border-radius: 12px; border-top-left-radius: 2px;
                    font-size: 0.95rem; color: var(--text-primary); white-space: pre-wrap; word-break: break-word;
                    box-shadow: 0 1px 2px rgba(0,0,0,0.05); line-height: 1.5;
                }
                .message-item--self .message-item__content { 
                    background: var(--primary-color); color: white; 
                    border-radius: 12px; border-top-right-radius: 2px; border-top-left-radius: 12px;
                }
                
                .collab-input-area { padding: 1.5rem; background: var(--bg-secondary); border-top: 1px solid var(--border-color); }
                .input-wrapper { display: flex; gap: 1rem; align-items: flex-end; }
                .collab-input { 
                    flex: 1; background: var(--bg-tertiary); border: 1px solid var(--border-color); 
                    border-radius: 8px; padding: 12px; color: var(--text-primary); font-size: 0.95rem;
                    resize: none; min-height: 50px; max-height: 150px; transition: border-color 0.2s;
                }
                .collab-input:focus { border-color: var(--primary-color); outline: none; }
                .collab-send { width: 50px; height: 50px; display: flex; align-items: center; justify-content: center; border-radius: 8px; }
                .input-hint { font-size: 0.75rem; color: var(--text-muted); margin: 0.5rem 0 0 0; text-align: right; }
            `}</style>
        </div>
    );
}
