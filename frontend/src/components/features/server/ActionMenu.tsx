import { useState, useEffect, useRef } from "react";
import { MoreVertical } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";

interface ActionItem {
    label: string;
    icon?: React.ElementType; // Lucide icon
    onClick: () => void;
    variant?: "default" | "danger" | "warning";
}

interface ActionMenuProps {
    actions: ActionItem[];
}

export default function ActionMenu({ actions }: ActionMenuProps) {
    const { t } = useLanguage();
    const [isOpen, setIsOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const [position, setPosition] = useState({ top: 0, right: 0 });

    // Calculate position on open
    useEffect(() => {
        if (isOpen && menuRef.current) {
            const rect = menuRef.current.getBoundingClientRect();
            setPosition({
                top: rect.bottom + 4, // 4px margin
                right: window.innerWidth - rect.right
            });
        }
    }, [isOpen]);

    // Close on click outside or scroll
    useEffect(() => {
        function handleInteraction(event: Event) {
            if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        }
        if (isOpen) {
            document.addEventListener("mousedown", handleInteraction);
            document.addEventListener("scroll", () => setIsOpen(false), true); // Close on scroll
        }
        return () => {
            document.removeEventListener("mousedown", handleInteraction);
            document.removeEventListener("scroll", () => setIsOpen(false), true);
        };
    }, [isOpen]);

    return (
        <div className="action-menu-container" ref={menuRef}>
            <button
                onClick={() => setIsOpen(!isOpen)}
                className={`btn btn--icon btn--ghost ${isOpen ? "active" : ""}`}
                title={t("common.actions")}
            >
                <MoreVertical size={16} />
            </button>

            {isOpen && (
                <div 
                    className="action-menu-dropdown" 
                    style={{ top: position.top, right: position.right }}
                >
                    {actions.map((action, index) => (
                        <button
                            key={index}
                            onClick={() => {
                                action.onClick();
                                setIsOpen(false);
                            }}
                            className={`action-menu-item ${action.variant || "default"}`}
                        >
                            {action.icon && <action.icon size={14} className="action-icon" />}
                            <span>{action.label}</span>
                        </button>
                    ))}
                </div>
            )}

            <style>{`
                .action-menu-container {
                    position: relative;
                    display: inline-block;
                }
                .action-menu-dropdown {
                    position: fixed;
                    background: var(--bg-surface, #1e1e1e);
                    border: 1px solid var(--border-color, #333);
                    border-radius: 6px;
                    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
                    min-width: 160px;
                    z-index: 9999;
                    padding: 4px;
                }
                .action-menu-item {
                    display: flex;
                    align-items: center;
                    width: 100%;
                    padding: 8px 12px;
                    border: none;
                    background: none;
                    color: var(--text-primary, #fff);
                    text-align: left;
                    font-size: 0.9rem;
                    cursor: pointer;
                    border-radius: 4px;
                    gap: 8px;
                    transition: background 0.1s;
                }
                .action-menu-item:hover {
                    background: var(--bg-hover, rgba(255,255,255,0.1));
                }
                .action-menu-item.danger { color: var(--color-danger, #ef4444); }
                .action-menu-item.danger:hover { background: rgba(239, 68, 68, 0.1); }
                .action-menu-item.warning { color: var(--color-warning, #f59e0b); }
                
                .action-icon { opacity: 0.8; }
            `}</style>
        </div>
    );
}
