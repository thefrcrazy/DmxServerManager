import { useState, useRef, useEffect } from 'react';
import { Outlet, Navigate, Link } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import Sidebar from './Sidebar';
import {
    LogOut,
    ChevronDown,
    ChevronLeft,
    Bell,
    UserCog
} from 'lucide-react';

export default function Layout() {
    const { user, logout } = useAuth();
    const { title, subtitle, backLink, headerActions } = usePageTitle();
    const [isUserMenuOpen, setIsUserMenuOpen] = useState(false);
    const [isSidebarCollapsed, setIsSidebarCollapsed] = useState(false);
    const menuRef = useRef<HTMLDivElement>(null);

    // Close menu when clicking outside
    useEffect(() => {
        const handleClickOutside = (event: MouseEvent) => {
            if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
                setIsUserMenuOpen(false);
            }
        };

        document.addEventListener('mousedown', handleClickOutside);
        return () => document.removeEventListener('mousedown', handleClickOutside);
    }, []);

    // Load sidebar state from localStorage
    useEffect(() => {
        const saved = localStorage.getItem('sidebarCollapsed');
        if (saved) {
            setIsSidebarCollapsed(saved === 'true');
        }
    }, []);

    const toggleSidebar = () => {
        const newState = !isSidebarCollapsed;
        setIsSidebarCollapsed(newState);
        localStorage.setItem('sidebarCollapsed', String(newState));
    };

    if (!user) {
        return <Navigate to="/login" replace />;
    }

    return (
        <div className={`layout ${isSidebarCollapsed ? 'layout--sidebar-collapsed' : ''}`}>
            <Sidebar isCollapsed={isSidebarCollapsed} onToggle={toggleSidebar} />

            {/* Topbar */}
            <header className={`topbar ${isSidebarCollapsed ? 'topbar--sidebar-collapsed' : ''}`}>
                <div className="topbar__left">
                    {/* Back Button */}
                    {backLink && (
                        <Link
                            to={backLink.to}
                            state={backLink.state}
                            className="topbar__back-btn"
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
                    {/* Notifications */}
                    <button className="topbar__icon-btn" title="Notifications">
                        <Bell size={20} />
                    </button>

                    {/* User Menu */}
                    <div className="user-menu" ref={menuRef}>
                        <button
                            className="user-menu__trigger"
                            onClick={() => setIsUserMenuOpen(!isUserMenuOpen)}
                        >
                            <div className="user-menu__avatar">
                                {user.username.charAt(0).toUpperCase()}
                            </div>
                            <ChevronDown size={16} className={`user-menu__chevron ${isUserMenuOpen ? 'user-menu__chevron--open' : ''}`} />
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
                                            {user.role === 'admin' ? 'Administrateur' : 'Utilisateur'}
                                        </span>
                                    </div>
                                </div>

                                <div className="user-menu__divider"></div>

                                <Link to="/user-settings" className="user-menu__item" onClick={() => setIsUserMenuOpen(false)}>
                                    <UserCog size={18} />
                                    <span>Mon Compte</span>
                                </Link>

                                <div className="user-menu__divider"></div>

                                <button className="user-menu__item user-menu__item--danger" onClick={logout}>
                                    <LogOut size={18} />
                                    <span>Déconnexion</span>
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
