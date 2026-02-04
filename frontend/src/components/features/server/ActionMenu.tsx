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

    // Close on click outside
    useEffect(() => {
        function handleClickOutside(event: MouseEvent) {
            if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
                setIsOpen(false);
            }
        }
        if (isOpen) {
            document.addEventListener("mousedown", handleClickOutside);
        }
        return () => {
            document.removeEventListener("mousedown", handleClickOutside);
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
                <div className="action-menu-dropdown">
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
                    position: absolute;
                    right: 0;
                    top: 100%;
                    margin-top: 4px;
                    background: var(--bg-surface, #1e1e1e);
                    border: 1px solid var(--border-color, #333);
                    border-radius: 6px;
                    box-shadow: 0 4px 12px rgba(0,0,0,0.3);
                    min-width: 160px;
                    z-index: 50;
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
