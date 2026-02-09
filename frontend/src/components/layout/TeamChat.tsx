import { useState, useEffect, useRef } from "react";
import { MessageSquare, Send, X, Trash2 } from "lucide-react";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip } from "@/components/ui";
import Ansi from "ansi-to-react";

interface Message {
    id: string;
    user_id: string;
    username: string;
    content: string;
    type_name: string;
    is_deleted: number;
    created_at: string;
    accent_color?: string;
}

export default function TeamChat({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
    const { user } = useAuth();
    const { t, language } = useLanguage();
    const [messages, setMessages] = useState<Message[]>([]);
    const [newMessage, setNewMessage] = useState("");
    const [isAnimating, setIsAnimating] = useState(false);
    const [shouldRender, setShouldRender] = useState(isOpen);
    const scrollRef = useRef<HTMLDivElement>(null);

    // Locale for time formatting
    const timeLocale = language === "fr" ? "fr-FR" : "en-US";

    // Sync rendering state with isOpen and handle animations
    useEffect(() => {
        if (isOpen) {
            setShouldRender(true);
            setIsAnimating(false);
        } else if (shouldRender) {
            setIsAnimating(true);
            const timer = setTimeout(() => {
                setShouldRender(false);
                setIsAnimating(false);
            }, 300); // Match SCSS animation duration
            return () => clearTimeout(timer);
        }
    }, [isOpen, shouldRender]);

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
        if (isOpen) {
            fetchMessages();
            const interval = setInterval(fetchMessages, 5000);
            return () => clearInterval(interval);
        }
    }, [isOpen]);

    useEffect(() => {
        if (scrollRef.current) {
            scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
    }, [messages, isOpen, shouldRender]);

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
                    msg_type: "chat"
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

    const handleDelete = async (id: string) => {
        try {
            const response = await fetch(`/api/v1/collaboration/messages/${id}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${localStorage.getItem("token")}` },
            });

            if (response.ok) {
                fetchMessages();
            }
        } catch (error) {
            console.error("Failed to delete message", error);
        }
    };

    if (!shouldRender) return null;

    return (
        <>
            {isOpen && <div className="team-chat-overlay" onClick={onClose} />}
            <div className={`team-chat ${isAnimating ? "team-chat--closing" : ""}`}>
                {/* Header */}
                <div className="team-chat__header">
                    <div className="team-chat__title">
                        <MessageSquare size={18} style={{ color: "var(--color-accent)" }} />
                        <span>Team Chat</span>
                    </div>
                    <button className="btn btn--icon btn--ghost team-chat__close" onClick={onClose}>
                        <X size={20} />
                    </button>
                </div>

                {/* Content / Messages */}
                <div className="team-chat__content" ref={scrollRef}>
                    {messages.length === 0 ? (
                        <div className="team-chat__empty">
                            <MessageSquare size={32} />
                            <p>Aucun message.</p>
                        </div>
                    ) : (
                        messages.map((msg) => (
                            <div key={msg.id} className={`chat-message ${msg.is_deleted ? "chat-message--deleted" : ""}`}>
                                <div className="chat-message__avatar" style={{ backgroundColor: msg.is_deleted ? "var(--color-bg-elevated)" : (msg.accent_color || "var(--color-accent)") }}>
                                    {msg.is_deleted ? "?" : msg.username.charAt(0)}
                                </div>
                                <div className="chat-message__body">
                                    <div className="chat-message__header">
                                        <div className="chat-message__meta">
                                            <span className="chat-message__username" style={{ color: msg.is_deleted ? "var(--color-text-muted)" : (msg.accent_color || "var(--color-accent)") }}>
                                                {msg.is_deleted ? t("common.system") : msg.username}
                                            </span>
                                            <span className="chat-message__time">
                                                {new Date(msg.created_at).toLocaleTimeString(timeLocale, { hour: "2-digit", minute: "2-digit" })}
                                            </span>
                                        </div>
                                        
                                        {/* Inline Actions */}
                                        {!msg.is_deleted && (user?.role === "admin" || user?.id === msg.user_id) && (
                                            <div className="chat-message__actions">
                                                <button className="action-btn action-btn--danger" onClick={() => handleDelete(msg.id)} title={t("common.delete")}>
                                                    <Trash2 size={14} />
                                                </button>
                                            </div>
                                        )}
                                    </div>
                                    <div className="chat-message__text">
                                        {msg.is_deleted ? (
                                            <span className="text-deleted">{t("collaboration.message_deleted")}</span>
                                        ) : (
                                            <Ansi>{msg.content}</Ansi>
                                        )}
                                    </div>
                                </div>
                            </div>
                        ))
                    )}
                </div>

                {/* Input Area */}
                <form className="team-chat__input-area" onSubmit={handleSend}>
                    <div className="team-chat__input-container">
                        <textarea 
                            className="input team-chat__input"
                            placeholder="Écrire un message..."
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
                            <button 
                                className="btn btn--primary btn--icon team-chat__send" 
                                type="submit" 
                                disabled={!newMessage.trim()}
                            >
                                <Send size={18} />
                            </button>
                        </Tooltip>
                    </div>
                </form>
            </div>
        </>
    );
}
