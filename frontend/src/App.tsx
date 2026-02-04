import { Routes, Route, Navigate, useNavigate, useLocation } from 'react-router-dom';
import { useState, useEffect } from 'react';
import { ToastProvider } from './contexts/ToastContext';
import { DialogProvider } from './contexts/DialogContext';
import ToastContainer from './components/common/ToastContainer';
import DialogContainer from './components/common/DialogContainer';
import { AuthProvider } from './contexts/AuthContext';
import { ThemeProvider } from './contexts/ThemeContext';
import { LanguageProvider } from './contexts/LanguageContext';
import { PageTitleProvider } from './contexts/PageTitleContext';
import Layout from './components/Layout';
import Login from './pages/Login';
import Dashboard from './pages/Dashboard';
import Servers from './pages/Servers';
import ServerDetail from './pages/ServerDetail';
import Backups from './pages/Backups';
import PanelSettings from './pages/PanelSettings';
import UserSettings from './pages/UserSettings';
import Setup from './pages/Setup';
import CreateServer from './pages/CreateServer';
import EditUser from './pages/EditUser';

function SetupCheck({ children }: { children: React.ReactNode }) {
    const [isChecking, setIsChecking] = useState(true);
    const navigate = useNavigate();
    const location = useLocation();

    useEffect(() => {
        const checkSetup = async () => {
            try {
                const res = await fetch('/api/v1/setup/status');
                if (res.ok) {
                    const data = await res.json();
                    if (!data.is_setup) {
                        if (location.pathname !== '/setup') {
                            navigate('/setup');
                        }
                    } else {
                        if (location.pathname === '/setup') {
                            navigate('/login');
                        }
                    }
                }
            } catch (error) {
                console.error('Setup check failed:', error);
            } finally {
                setIsChecking(false);
            }
        };

        checkSetup();
    }, []);

    if (isChecking) {
        return (
            <div className="loading-screen">
                <div className="spinner"></div>
            </div>
        );
    }

    return <>{children}</>;
}

function App() {
    return (
        <ThemeProvider>
            <ToastProvider>
                <DialogProvider>
                    <ToastContainer />
                    <DialogContainer />
                    <LanguageProvider>
                        <AuthProvider>
                            <PageTitleProvider>
                                <SetupCheck>
                                    <Routes>
                                        <Route path="/setup" element={<Setup />} />
                                        <Route path="/login" element={<Login />} />
                                        <Route element={<Layout />}>
                                            <Route path="/dashboard" element={<Dashboard />} />
                                            <Route path="/servers" element={<Servers />} />
                                            <Route path="/servers/create" element={<CreateServer />} />
                                            <Route path="/servers/:id" element={<ServerDetail />} />
                                            <Route path="/backups" element={<Backups />} />
                                            <Route path="/panel-settings" element={<PanelSettings />} />
                                            <Route path="/panel-settings/users/:id" element={<EditUser />} />
                                            <Route path="/user-settings" element={<UserSettings />} />
                                            {/* Redirect old route */}
                                            <Route path="/settings" element={<Navigate to="/panel-settings" replace />} />
                                        </Route>
                                        <Route path="/" element={<Navigate to="/dashboard" replace />} />
                                    </Routes>
                                </SetupCheck>
                            </PageTitleProvider>
                        </AuthProvider>
                    </LanguageProvider>
                </DialogProvider>
            </ToastProvider>
        </ThemeProvider>
    );
}

export default App;
