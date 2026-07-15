import { useState, useEffect, useRef } from "react";
import { Link, useLocation } from "react-router-dom";
import { Tooltip } from "@/components/ui";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useTheme } from "@/contexts/ThemeContext";
import { apiService } from "@/services";
import {
    LayoutDashboard,
    Server,
    Bell,
    ChevronsLeft,
    MessageSquareText,
    ListChecks,
    UsersRound,
    X,
} from "lucide-react";

interface SidebarProps {
    isCollapsed: boolean;
    isMobileOpen: boolean;
    onToggle: () => void;
    onMobileClose: () => void;
}

export default function Sidebar({ isCollapsed, isMobileOpen, onToggle, onMobileClose }: SidebarProps) {
    const location = useLocation();
    const [version, setVersion] = useState<string>("");
    const { t } = useLanguage();
    const { user } = useAuth();
    const { activeTheme } = useTheme();
    const logoUrl = activeTheme.assets.logo?.url ?? "/dmx-server-manager-logo.png";
    const mobileCloseRef = useRef<HTMLButtonElement>(null);
    const canReadUsers = user?.permissions.includes("*") || user?.permissions.includes("user.read");
    const canManageProfiles = user?.permissions.includes("*") || user?.permissions.includes("profile.manage");
    const canReadChat = user?.permissions.includes("*") || user?.permissions.includes("chat.read");
    const canReadNotifications = user?.permissions.includes("*") || user?.permissions.includes("notifications.read");
    const canReadJobs = user?.permissions.includes("*") || user?.permissions.includes("job.read");

    const navItems = [
        { icon: LayoutDashboard, label: t("sidebar.dashboard"), path: "/dashboard" },
        { icon: Server, label: t("sidebar.servers"), path: "/servers" },
        ...(canReadJobs ? [{ icon: ListChecks, label: t("sidebar.jobs"), path: "/jobs" }] : []),
        ...(canReadChat ? [{ icon: MessageSquareText, label: t("sidebar.chat"), path: "/chat" }] : []),
        ...(canReadNotifications ? [{ icon: Bell, label: t("sidebar.notifications"), path: "/notifications" }] : []),
        ...(canReadUsers || canManageProfiles ? [{ icon: UsersRound, label: t("sidebar.administration"), path: "/administration" }] : []),
    ];

    useEffect(() => {
        fetchVersion();
    }, []);

    useEffect(() => {
        if (!isMobileOpen) return;
        let focusFrame = 0;
        const visibilityFrame = requestAnimationFrame(() => {
            focusFrame = requestAnimationFrame(() => mobileCloseRef.current?.focus());
        });
        return () => {
            cancelAnimationFrame(visibilityFrame);
            if (focusFrame) cancelAnimationFrame(focusFrame);
        };
    }, [isMobileOpen]);

    const fetchVersion = async () => {
        try {
            const response = await apiService.system.health();
            if (response.success) {
                setVersion(response.data.version);
            }
        } catch (error) {
            // Internal log, keeping in english or neutral
            console.error("Failed to load version:", error);
            setVersion("1.0.0");
        }
    };

    return (
        <aside id="app-sidebar" className={`sidebar ${isCollapsed ? "sidebar--collapsed" : ""} ${isMobileOpen ? "open" : ""}`}>
            <button
                ref={mobileCloseRef}
                type="button"
                className="sidebar__mobile-close"
                aria-label={t("sidebar.close_mobile_menu")}
                onClick={onMobileClose}
            >
                <X size={20} aria-hidden="true" />
            </button>
            {/* Toggle Button - Always first when collapsed */}
            <Tooltip content={isCollapsed ? t("sidebar.expand_menu") : t("sidebar.collapse_menu")} position="right">
                <button
                    className="sidebar__toggle"
                    onClick={onToggle}
                    aria-label={isCollapsed ? t("sidebar.expand_menu") : t("sidebar.collapse_menu")}
                    aria-expanded={!isCollapsed}
                >
                    <ChevronsLeft size={16} />
                </button>
            </Tooltip>

            <div className="sidebar__header">
                <Link to="/" className="sidebar__logo-link" onClick={onMobileClose}>
                    {isCollapsed ? (
                        <img
                            src={logoUrl}
                            alt="DmxServerManager"
                            className="sidebar__logo sidebar__logo--small"
                        />
                    ) : (
                        <img
                            src={logoUrl}
                            alt="DmxServerManager"
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
                            onClick={onMobileClose}
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
