import { lazy, ReactNode, Suspense, useEffect, useState } from "react";
import { Navigate, Outlet, Route, Routes, useLocation, useNavigate } from "react-router-dom";
import { DialogContainer, LoadingScreen, ToastContainer } from "@/components/shared";
import { Layout } from "@/components/layout";
import { AuthProvider, useAuth } from "@/contexts/AuthContext";
import { DialogProvider } from "@/contexts/DialogContext";
import { LanguageProvider } from "@/contexts/LanguageContext";
import { PageTitleProvider } from "@/contexts/PageTitleContext";
import { ThemeProvider } from "@/contexts/ThemeContext";
import { ToastProvider } from "@/contexts/ToastContext";
import { apiService } from "@/services";

const Login = lazy(() => import("@/pages/Login"));
const Dashboard = lazy(() => import("@/pages/Dashboard"));
const Servers = lazy(() => import("@/pages/Servers"));
const ServerDetail = lazy(() => import("@/pages/ServerDetail"));
const UserSettings = lazy(() => import("@/pages/UserSettings"));
const Setup = lazy(() => import("@/pages/Setup"));
const CreateServer = lazy(() => import("@/pages/CreateServer"));
const Administration = lazy(() => import("@/pages/Administration"));
const Chat = lazy(() => import("@/pages/Chat"));
const Notifications = lazy(() => import("@/pages/Notifications"));
const Jobs = lazy(() => import("@/pages/Jobs"));
const MandatoryPasswordChange = lazy(() => import("@/pages/MandatoryPasswordChange"));

function RequireReadySession() {
    const { user, isLoading } = useAuth();
    if (isLoading) return <LoadingScreen />;
    if (!user) return <Navigate to="/login" replace />;
    if (user.must_change_password) return <Navigate to="/change-password" replace />;
    return <Outlet />;
}

function RequirePasswordChange() {
    const { user, isLoading } = useAuth();
    if (isLoading) return <LoadingScreen />;
    if (!user) return <Navigate to="/login" replace />;
    if (!user.must_change_password) return <Navigate to="/dashboard" replace />;
    return <Outlet />;
}

function SetupCheck({ children }: { children: ReactNode }) {
    const [isChecking, setIsChecking] = useState(true);
    const navigate = useNavigate();
    const location = useLocation();

    useEffect(() => {
        let active = true;
        void apiService.auth.checkAuthStatus().then((response) => {
            if (!active || !response.success) return;
            if (response.data.needs_setup && location.pathname !== "/setup") navigate("/setup", { replace: true });
            if (!response.data.needs_setup && location.pathname === "/setup") navigate("/login", { replace: true });
        }).finally(() => active && setIsChecking(false));
        return () => { active = false; };
    }, [location.pathname, navigate]);

    return isChecking ? <LoadingScreen /> : children;
}

export default function App() {
    return (
        <LanguageProvider>
            <ToastProvider>
                <DialogProvider>
                    <ToastContainer />
                    <DialogContainer />
                    <AuthProvider>
                        <ThemeProvider>
                            <PageTitleProvider>
                                <SetupCheck>
                                    <Suspense fallback={<LoadingScreen />}>
                                        <Routes>
                                            <Route path="/setup" element={<Setup />} />
                                            <Route path="/login" element={<Login />} />
                                            <Route element={<RequirePasswordChange />}>
                                                <Route path="/change-password" element={<MandatoryPasswordChange />} />
                                            </Route>
                                            <Route element={<RequireReadySession />}>
                                                <Route element={<Layout />}>
                                                    <Route path="/dashboard" element={<Dashboard />} />
                                                    <Route path="/servers" element={<Servers />} />
                                                    <Route path="/servers/create" element={<CreateServer />} />
                                                    <Route path="/servers/:id" element={<ServerDetail />} />
                                                    <Route path="/jobs" element={<Jobs />} />
                                                    <Route path="/chat" element={<Chat />} />
                                                    <Route path="/notifications" element={<Notifications />} />
                                                    <Route path="/administration" element={<Administration />} />
                                                    <Route path="/user-settings" element={<UserSettings />} />
                                                    <Route path="/settings" element={<Navigate to="/user-settings" replace />} />
                                                </Route>
                                            </Route>
                                            <Route path="/" element={<Navigate to="/dashboard" replace />} />
                                            <Route path="*" element={<Navigate to="/dashboard" replace />} />
                                        </Routes>
                                    </Suspense>
                                </SetupCheck>
                            </PageTitleProvider>
                        </ThemeProvider>
                    </AuthProvider>
                </DialogProvider>
            </ToastProvider>
        </LanguageProvider>
    );
}
