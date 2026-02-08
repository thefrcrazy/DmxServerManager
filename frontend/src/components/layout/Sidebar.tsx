import { useState, useEffect } from "react";
import { Link, useLocation } from "react-router-dom";
import { Tooltip } from "@/components/ui";
import {
    LayoutDashboard,
    Server,
    Settings,
    HardDrive,
    ChevronsLeft,
    MessageSquare,
} from "lucide-react";

interface SidebarProps {
    isCollapsed: boolean;
    onToggle: () => void;
}

import { useLanguage } from "@/contexts/LanguageContext";
import { apiService } from "@/services";

export default function Sidebar({ isCollapsed, onToggle }: SidebarProps) {
    const location = useLocation();
    const [version, setVersion] = useState<string>("");
    const { t } = useLanguage();

    const navItems = [
        { icon: LayoutDashboard, label: t("sidebar.dashboard"), path: "/dashboard" },
        { icon: Server, label: t("sidebar.servers"), path: "/servers" },
        { icon: HardDrive, label: t("sidebar.backups"), path: "/backups" },
        { icon: Settings, label: t("sidebar.settings"), path: "/panel-settings" },
    ];

    useEffect(() => {
        fetchVersion();
    }, []);

    const fetchVersion = async () => {
        try {
            const response = await apiService.system.getSettings();
            if (response.success) {
                setVersion(response.data.version || "0.1.0");
            }
        } catch (error) {
            // Internal log, keeping in english or neutral
            console.error("Failed to load version:", error);
            setVersion("0.1.0");
        }
    };

    return (
        <aside className={`sidebar ${isCollapsed ? "sidebar--collapsed" : ""}`}>
            {/* Toggle Button - Always first when collapsed */}
            <Tooltip content={isCollapsed ? t("sidebar.expand_menu") : t("sidebar.collapse_menu")} position="right">
                <button
                    className="sidebar__toggle"
                    onClick={onToggle}
                >
                    <ChevronsLeft size={16} />
                </button>
            </Tooltip>

            <div className="sidebar__header">
                <Link to="/" className="sidebar__logo-link">
                    {isCollapsed ? (
                        <img
                            src="/draveur-manager-logo.png"
                            alt="Draveur"
                            className="sidebar__logo sidebar__logo--small"
                        />
                    ) : (
                        <img
                            src="/draveur-manager-logo.png"
                            alt="Draveur Manager"
                            className="sidebar__logo sidebar__logo--full"
                        />
                    )}
                </Link>
            </div>

            <nav className="sidebar__nav">
                {navItems.map((item) => (
                    <Tooltip
                        key={item.path}
                        content={item.label}
                        position="right"
                        disabled={!isCollapsed}
                    >
                        <Link
                            to={item.path}
                            className={`sidebar__link ${location.pathname.startsWith(item.path) ? "active" : ""}`}
                        >
                            <item.icon size={20} />
                            <span className="sidebar__label">{item.label}</span>
                        </Link>
                    </Tooltip>
                ))}
            </nav>

            {/* Version footer */}
            <div className="sidebar__footer">
                <Tooltip content={`${t("sidebar.version")} ${version}`} position="right" disabled={!isCollapsed}>
                    <span className="sidebar__version">
                        {isCollapsed ? `v${version.split(".")[0]}` : `v${version}`}
                    </span>
                </Tooltip>
            </div>
        </aside>
    );
}
