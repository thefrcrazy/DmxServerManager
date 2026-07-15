import { FormEvent, useState } from "react";
import { AlertCircle, ShieldCheck } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { apiService } from "@/services";

export default function Setup() {
    const { t } = useLanguage();
    const { refreshSession } = useAuth();
    const navigate = useNavigate();
    const [error, setError] = useState("");
    const [isLoading, setIsLoading] = useState(false);

    const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        const form = new FormData(event.currentTarget);
        const username = String(form.get("username") ?? "").trim();
        const password = String(form.get("password") ?? "");
        const confirmation = String(form.get("confirmation") ?? "");
        const setupToken = String(form.get("setup_token") ?? "").trim();
        if (password.length < 12) {
            setError(t("user_settings.password_min_length"));
            return;
        }
        if (password !== confirmation) {
            setError(t("user_settings.password_mismatch"));
            return;
        }
        setIsLoading(true);
        setError("");
        const response = await apiService.auth.setup(username, password, setupToken || undefined);
        if (!response.success) {
            setError(response.error.message || t("common.error"));
            setIsLoading(false);
            return;
        }
        await refreshSession();
        navigate("/dashboard", { replace: true });
    };

    return (
        <main className="setup-page">
            <section className="setup-wizard" aria-labelledby="setup-title">
                <div className="setup-wizard__header">
                    <img src="/dmx-server-manager-logo.png" alt="" className="setup-wizard__logo" />
                    <h1 id="setup-title" className="setup-wizard__title">DmxServerManager</h1>
                    <p className="setup-wizard__subtitle">{t("setup.owner_subtitle")}</p>
                </div>
                {error && <div className="alert alert--danger" role="alert"><AlertCircle size={16} />{error}</div>}
                <form onSubmit={handleSubmit} className="login-form">
                    <div className="form-group">
                        <label className="form-label" htmlFor="setup-username">{t("setup.username")}</label>
                        <input id="setup-username" name="username" className="form-input" autoComplete="username" required />
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="setup-password">{t("setup.password")}</label>
                        <input id="setup-password" name="password" type="password" className="form-input" autoComplete="new-password" required minLength={12} />
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="setup-confirmation">{t("auth.confirm_password")}</label>
                        <input id="setup-confirmation" name="confirmation" type="password" className="form-input" autoComplete="new-password" required minLength={12} />
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="setup-token">{t("setup.remote_token")}</label>
                        <input id="setup-token" name="setup_token" type="password" className="form-input" autoComplete="off" />
                        <p className="form-hint">{t("setup.remote_token_hint")}</p>
                    </div>
                    <button type="submit" className="btn btn--primary btn--lg btn--full" disabled={isLoading}>
                        {isLoading ? <span className="spinner spinner--sm" /> : <ShieldCheck size={18} />}
                        {t("setup.finish")}
                    </button>
                </form>
            </section>
        </main>
    );
}
