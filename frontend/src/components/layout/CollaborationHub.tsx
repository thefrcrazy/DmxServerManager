import { useState, useEffect, useRef } from "react";
import { MessageSquare, StickyNote, Send, X } from "lucide-react";
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
    created_at: string;
    accent_color?: string;
}

export default function CollaborationHub({ isOpen, onClose }: { isOpen: boolean; onClose: () => void }) {
    const { user } = useAuth();
    const { t } = useLanguage();
    const [activeTab, setActiveTab] = useState<"chat" | "notes">("chat");
    const [messages, setMessages] = useState<Message[]>([]);
    const [newMessage, setNewMessage] = useState("");
    const [isAnimating, setIsAnimating] = useState(false);
    const [shouldRender, setShouldRender] = useState(isOpen);
    const scrollRef = useRef<HTMLDivElement>(null);

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
            }, 400); // Match SCSS animation duration
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
    }, [messages, activeTab, isOpen, shouldRender]);

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

    if (!shouldRender) return null;

    const filteredMessages = messages.filter(m => m.type_name === activeTab);

    return (
        <>
            {isOpen && <div className="collab-overlay" onClick={onClose} />}
            <div className={`collab-hub ${isAnimating ? "collab-hub--closing" : ""}`}>
                {/* Header */}
                <div className="collab-hub__header">
                    <div className="collab-hub__title">
                        <MessageSquare size={18} style={{ color: "var(--color-accent)" }} />
                        <span>Collaboration Hub</span>
                    </div>
                    <button className="btn btn--icon btn--ghost collab-hub__close" onClick={onClose}>
                        <X size={20} />
                    </button>
                </div>

                {/* Navigation Pills Pattern */}
                <div className="collab-hub__nav">
                    <button 
                        className={`nav-pill ${activeTab === "chat" ? "active" : ""}`}
                        onClick={() => setActiveTab("chat")}
                    >
                        <MessageSquare size={14} />
                        <span>Chat d'équipe</span>
                    </button>
                    <button 
                        className={`nav-pill ${activeTab === "notes" ? "active" : ""}`}
                        onClick={() => setActiveTab("notes")}
                    >
                        <StickyNote size={14} />
                        <span>Notes</span>
                    </button>
                </div>

                {/* Content / Messages */}
                <div className="collab-hub__content" ref={scrollRef}>
                    {filteredMessages.length === 0 ? (
                        <div className="collab-hub__empty">
                            <MessageSquare size={32} />
                            <p>Aucun message pour le moment.</p>
                        </div>
                    ) : (
                        filteredMessages.map((msg) => (
                            <div key={msg.id} className={`message-item ${msg.user_id === user?.id ? "message-item--self" : ""}`}>
                                <div className="message-item__header">
                                    <span className="message-item__user" style={{ color: msg.accent_color || "var(--color-accent)" }}>{msg.username}</span>
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

                {/* Input Area */}
                <form className="collab-hub__input-area" onSubmit={handleSend}>
                    <div className="collab-hub__input-container">
                        <textarea 
                            className="input collab-hub__input"
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
                            <button 
                                className="btn btn--primary btn--icon collab-hub__send" 
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