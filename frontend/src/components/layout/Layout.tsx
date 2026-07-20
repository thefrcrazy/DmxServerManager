import { type CSSProperties, useState, useRef, useEffect, useCallback } from "react";
import { Outlet, Navigate, Link, useLocation } from "react-router-dom";
import { useAuth } from "@/contexts/AuthContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { roleLabel } from "@/utils/roles";
import Sidebar from "./Sidebar";
import {
    LogOut,
    ChevronDown,
    ChevronLeft,
    Menu,
    UserCog,
} from "lucide-react";

export default function Layout() {
    const { user, logout, isLoading } = useAuth();
    const { title, subtitle, backLink, headerActions } = usePageTitle();
    const { t } = useLanguage();
    const location = useLocation();
    const [isUserMenuOpen, setIsUserMenuOpen] = useState(false);
    const [sidebarWidth, setSidebarWidth] = useState(232);
    const [isMobileSidebarOpen, setIsMobileSidebarOpen] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);
    const mobileMenuRef = useRef<HTMLButtonElement>(null);
    const previousPathnameRef = useRef(location.pathname);

    // Close menu when clicking outside
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
                setIsUserMenuOpen(false);
            }
        };

        document.addEventListener("mousedown", handleClickOutside);
        return () => document.removeEventListener("mousedown", handleClickOutside);
    }, []);

    useEffect(() => {
        const saved = Number(localStorage.getItem(`dmx_sidebar_width:${user?.id}`));
        setSidebarWidth(Number.isFinite(saved) && saved >= 200 && saved <= 400 ? saved : 232);
    }, [user?.id]);

    useEffect(() => {
        if (previousPathnameRef.current === location.pathname) return;
        previousPathnameRef.current = location.pathname;
        setIsMobileSidebarOpen(false);
    }, [location.pathname]);

    useEffect(() => {
        if (!isMobileSidebarOpen) return;
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key !== "Escape") return;
            setIsMobileSidebarOpen(false);
            mobileMenuRef.current?.focus();
        };
        document.addEventListener("keydown", closeOnEscape);
        return () => document.removeEventListener("keydown", closeOnEscape);
    }, [isMobileSidebarOpen]);

    const updateSidebarWidth = useCallback((width: number) => {
        const safeWidth = Math.min(400, Math.max(200, Math.round(width)));
        setSidebarWidth(safeWidth);
        if (user?.id) localStorage.setItem(`dmx_sidebar_width:${user.id}`, String(safeWidth));
    }, [user?.id]);

    if (isLoading) return <div className="loading-screen"><div className="spinner" /></div>;

    if (!user) {
        return <Navigate to="/login" replace />;
    }

    return (
        <div className="layout" style={{ "--sidebar-width": `${sidebarWidth}px` } as CSSProperties}>
            <Sidebar
                width={sidebarWidth}
                isMobileOpen={isMobileSidebarOpen}
                onWidthChange={updateSidebarWidth}
                onMobileClose={() => {
                    setIsMobileSidebarOpen(false);
                    mobileMenuRef.current?.focus();
                }}
            />
            {isMobileSidebarOpen && <button
                type="button"
                className="sidebar-backdrop"
                aria-label={t("sidebar.close_mobile_menu")}
                tabIndex={-1}
                onClick={() => setIsMobileSidebarOpen(false)}
            />}

            {/* Topbar */}
            <header className="topbar">
                <div className="topbar__left">
                    <button
                        ref={mobileMenuRef}
                        type="button"
                        className="topbar__mobile-menu"
                        aria-label={t("sidebar.open_mobile_menu")}
                        aria-controls="app-sidebar"
                        aria-expanded={isMobileSidebarOpen}
                        onClick={() => setIsMobileSidebarOpen(true)}
                    >
                        <Menu size={20} aria-hidden="true" />
                    </button>
                    {/* Back Button */}
                    {backLink && (
                        <Link
                            to={backLink.to}
                            state={backLink.state}
                            className="topbar__back-btn"
                            aria-label={t("common.back")}
                        >
                            <ChevronLeft size={20} />
                        </Link>
                    )}
                    {/* Page Title */}
                    {title && (
                        <div className="topbar__page-info">
                            <h1 className="topbar__title">{title}</h1>
                            {subtitle && <p className="topbar__subtitle">{subtitle}</p>}
                        </div>
                    )}
                </div>

                <div className="topbar__right">
                    {/* Header Actions (injected from pages) */}
                    {headerActions && (
                        <div className="topbar__actions">
                            {headerActions}
                        </div>
                    )}

                    {/* User Menu */}
                    <div className="user-menu" ref={menuRef}>
                        <button
                            className="user-menu__trigger"
                            onClick={() => setIsUserMenuOpen(!isUserMenuOpen)}
                            aria-label={t("sidebar.user_menu")}
                            aria-expanded={isUserMenuOpen}
                        >
                            <div className="user-menu__avatar">
                                {user.username.charAt(0).toUpperCase()}
                            </div>
                            <ChevronDown size={16} className={`user-menu__chevron ${isUserMenuOpen ? "user-menu__chevron--open" : ""}`} />
                        </button>

                        {isUserMenuOpen && (
                            <div className="user-menu__dropdown">
                                <div className="user-menu__header">
                                    <div className="user-menu__avatar user-menu__avatar--lg">
                                        {user.username.charAt(0).toUpperCase()}
                                    </div>
                                    <div className="user-menu__info">
                                        <span className="user-menu__name">{user.username}</span>
                                        <span className="user-menu__role">
                                            {roleLabel(user.role, undefined, t)}
                                        </span>
                                    </div>
                                </div>

                                <div className="user-menu__divider"></div>

                                <Link to="/user-settings" className="user-menu__item" onClick={() => setIsUserMenuOpen(false)}>
                                    <UserCog size={18} />
                                    <span>{t("sidebar.my_account")}</span>
                                </Link>

                                <div className="user-menu__divider"></div>

                                <button className="user-menu__item user-menu__item--danger" onClick={logout}>
                                    <LogOut size={18} />
                                    <span>{t("sidebar.logout")}</span>
                                </button>
                            </div>
                        )}
                    </div>
                </div>
            </header>

            {/* Main Content */}
            <main className="main-content">
                <div className="page-container">
                    <Outlet />
                </div>
            </main>
        </div>
    );
}
