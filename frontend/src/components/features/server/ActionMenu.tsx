import { useState, useEffect, useRef } from "react";
import { MoreVertical } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { Tooltip } from "@/components/ui";

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
            <Tooltip content={t("common.actions")} position="top">
                <button
                    onClick={() => setIsOpen(!isOpen)}
                    className={`btn btn--icon btn--ghost ${isOpen ? "active" : ""}`}
                >
                    <MoreVertical size={16} />
                </button>
            </Tooltip>

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
        </div>
    );
}