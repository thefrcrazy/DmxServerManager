import { FormEvent, useEffect, useState } from "react";
import { AlertCircle, LogIn } from "lucide-react";
import { Navigate, useLocation, useNavigate } from "react-router-dom";
import { PASSWORD_CHANGED_EVENT, PASSWORD_CHANGED_FLASH_KEY, useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";

export default function Login() {
    const { user, login } = useAuth();
    const { t } = useLanguage();
    const navigate = useNavigate();
    const location = useLocation();
    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");
    const [error, setError] = useState("");
    const [isLoading, setIsLoading] = useState(false);
    const [passwordChanged] = useState(() => (
        (location.state as { passwordChanged?: unknown } | null)?.passwordChanged === true
        || sessionStorage.getItem(PASSWORD_CHANGED_FLASH_KEY) === "1"
    ));

    const [passwordChangedVisible, setPasswordChangedVisible] = useState(passwordChanged);

    useEffect(() => {
        const revealPasswordChanged = () => setPasswordChangedVisible(true);
        window.addEventListener(PASSWORD_CHANGED_EVENT, revealPasswordChanged);
        return () => window.removeEventListener(PASSWORD_CHANGED_EVENT, revealPasswordChanged);
    }, []);

    const handleSubmit = async (event: FormEvent) => {
        event.preventDefault();
        setError("");
        setIsLoading(true);
        try {
            await login(username, password);
            sessionStorage.removeItem(PASSWORD_CHANGED_FLASH_KEY);
            navigate("/dashboard", { replace: true });
        } catch (loginError) {
            setError(loginError instanceof Error ? loginError.message : t("auth.login_failed"));
        } finally {
            setIsLoading(false);
        }
    };

    if (user) return <Navigate to={user.must_change_password ? "/change-password" : "/dashboard"} replace />;

    return (
        <main className="login-page">
            <section className="card login-card" aria-labelledby="login-title">
                <div className="login-header">
                    <img src="/dmx-server-manager-logo.png" alt="" className="login-header__logo" />
                    <h1 id="login-title">DmxServerManager</h1>
                    <p className="text-muted">{t("auth.login_subtitle")}</p>
                </div>
                {passwordChangedVisible && (
                    <div className="alert alert--success" role="status">
                        {t("password_change.success")}
                    </div>
                )}
                <form onSubmit={handleSubmit} className="login-form">
                    {error && <div className="alert alert--error" role="alert"><AlertCircle size={16} />{error}</div>}
                    <div className="form-group">
                        <label className="form-label" htmlFor="username">{t("auth.username")}</label>
                        <input id="username" className="form-input" value={username} onChange={(event) => setUsername(event.target.value)} autoComplete="username" required autoFocus />
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="password">{t("auth.password")}</label>
                        <input id="password" type="password" className="form-input" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" required />
                    </div>
                    <button type="submit" className="btn btn--primary btn--lg btn--full" disabled={isLoading}>
                        {isLoading ? <span className="spinner spinner--sm" /> : <LogIn size={18} />}
                        {t("auth.login")}
                    </button>
                </form>
                <footer className="login-footer"><p>DmxServerManager v1.1.4</p></footer>
            </section>
        </main>
    );
}
