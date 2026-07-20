import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useTheme } from "@/contexts/ThemeContext";
import { apiService } from "@/services";
import {
    Activity,
    LayoutDashboard,
    Server,
    Settings,
    X,
} from "lucide-react";

const MIN_WIDTH = 200;
const MAX_WIDTH = 400;
const DEFAULT_WIDTH = 232;

interface SidebarProps {
    width: number;
    isMobileOpen: boolean;
    onWidthChange: (width: number) => void;
    onMobileClose: () => void;
}

function boundedWidth(value: number): number {
    return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(value)));
}

export default function Sidebar({ width, isMobileOpen, onWidthChange, onMobileClose }: SidebarProps) {
    const location = useLocation();
    const [version, setVersion] = useState("");
    const { t } = useLanguage();
    const { user } = useAuth();
    const { activeTheme } = useTheme();
    const logoUrl = activeTheme.assets.logo?.url ?? "/dmx-server-manager-logo.png";
    const mobileCloseRef = useRef<HTMLButtonElement>(null);
    const resizeHandleRef = useRef<HTMLDivElement>(null);
    const canReadActivity = Boolean(user?.permissions.includes("*")
        || user?.permissions.includes("job.read")
        || user?.permissions.includes("audit.read"));
    const canOpenAdministration = Boolean(user?.permissions.includes("*")
        || user?.permissions.includes("user.read")
        || user?.permissions.includes("profile.manage")
        || user?.permissions.includes("panel.network.manage"));

    const navItems = [
        { icon: LayoutDashboard, label: t("sidebar.dashboard"), path: "/dashboard" },
        { icon: Server, label: t("sidebar.servers"), path: "/servers" },
        ...(canReadActivity ? [{ icon: Activity, label: t("sidebar.activity"), path: "/activity" }] : []),
        ...(canOpenAdministration ? [{ icon: Settings, label: t("sidebar.administration"), path: "/administration" }] : []),
    ];

    useEffect(() => {
        void apiService.system.health().then((response) => {
            if (response.success) setVersion(response.data.version);
        });
    }, []);

    useLayoutEffect(() => {
        if (!isMobileOpen) return;
        const focusClose = () => mobileCloseRef.current?.focus({ preventScroll: true });
        focusClose();
        const frame = requestAnimationFrame(focusClose);
        return () => cancelAnimationFrame(frame);
    }, [isMobileOpen]);

    const resizeFromPointer = (event: React.PointerEvent<HTMLDivElement>) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        onWidthChange(boundedWidth(event.clientX));
    };

    const resizeWithKeyboard = (event: React.KeyboardEvent<HTMLDivElement>) => {
        const step = event.shiftKey ? 24 : 8;
        let next: number | null = null;
        if (event.key === "ArrowLeft") next = width - step;
        if (event.key === "ArrowRight") next = width + step;
        if (event.key === "Home") next = MIN_WIDTH;
        if (event.key === "End") next = MAX_WIDTH;
        if (event.key === "Enter") next = DEFAULT_WIDTH;
        if (next === null) return;
        event.preventDefault();
        onWidthChange(boundedWidth(next));
    };

    return (
        <aside id="app-sidebar" className={`sidebar ${isMobileOpen ? "open" : ""}`} aria-label={t("sidebar.navigation")}>
            {isMobileOpen && <button
                ref={mobileCloseRef}
                type="button"
                className="sidebar__mobile-close"
                aria-label={t("sidebar.close_mobile_menu")}
                onClick={onMobileClose}
                autoFocus
            >
                <X size={20} aria-hidden="true" />
            </button>}

            <div className="sidebar__header">
                <Link to="/dashboard" className="sidebar__logo-link" onClick={onMobileClose}>
                    <img src={logoUrl} alt="DmxServerManager" className="sidebar__logo sidebar__logo--full" />
                </Link>
            </div>

            <nav className="sidebar__nav">
                {navItems.map((item) => (
                    <Link
                        key={item.path}
                        to={item.path}
                        className={`sidebar__link ${location.pathname.startsWith(item.path) ? "active" : ""}`}
                        onClick={onMobileClose}
                    >
                        <item.icon size={19} aria-hidden="true" />
                        <span className="sidebar__label">{item.label}</span>
                    </Link>
                ))}
            </nav>

            <div className="sidebar__footer">
                <span className="sidebar__version">v{version || "…"}</span>
            </div>

            <div
                ref={resizeHandleRef}
                className="sidebar__resize-handle"
                role="separator"
                aria-orientation="vertical"
                aria-label={t("sidebar.resize")}
                aria-valuemin={MIN_WIDTH}
                aria-valuemax={MAX_WIDTH}
                aria-valuenow={width}
                tabIndex={0}
                onPointerDown={resizeFromPointer}
                onPointerMove={(event) => {
                    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                        onWidthChange(boundedWidth(event.clientX));
                    }
                }}
                onKeyDown={resizeWithKeyboard}
            />
        </aside>
    );
}
