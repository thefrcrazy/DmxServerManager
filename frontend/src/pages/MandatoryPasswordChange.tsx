import { FormEvent, useState } from "react";
import { AlertCircle, KeyRound, LogOut } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { passwordMeetsPolicy } from "@/utils/password";

export default function MandatoryPasswordChange() {
    const { user, changePassword, logout } = useAuth();
    const { t } = useLanguage();
    const navigate = useNavigate();
    const [currentPassword, setCurrentPassword] = useState("");
    const [newPassword, setNewPassword] = useState("");
    const [confirmation, setConfirmation] = useState("");
    const [error, setError] = useState("");
    const [isSubmitting, setIsSubmitting] = useState(false);

    const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        setError("");
        if (newPassword !== confirmation) {
            setError(t("user_settings.password_mismatch"));
            return;
        }
        if (!passwordMeetsPolicy(newPassword)) {
            setError(t("user_settings.password_min_length"));
            return;
        }
        if (newPassword === currentPassword) {
            setError(t("user_settings.password_must_differ"));
            return;
        }

        setIsSubmitting(true);
        try {
            await changePassword(currentPassword, newPassword);
            navigate("/login", { replace: true, state: { passwordChanged: true } });
        } catch (changeError) {
            const message = changeError instanceof Error
                ? changeError.message
                : t("user_settings.password_error");
            setError(message.includes(".") ? t(message) : message);
        } finally {
            setIsSubmitting(false);
        }
    };

    const handleLogout = async () => {
        await logout();
        navigate("/login", { replace: true });
    };

    return (
        <main className="login-page">
            <section className="card mandatory-password-card" aria-labelledby="mandatory-password-title">
                <header className="mandatory-password-card__header">
                    <img src="/dmx-server-manager-logo.png" alt="" className="login-header__logo" />
                    <h1 id="mandatory-password-title">{t("password_change.title")}</h1>
                    <p>{t("password_change.description")}</p>
                    <p className="text-muted">
                        {t("password_change.signed_in_as")} <strong>{user?.username}</strong>
                    </p>
                </header>

                {error && (
                    <div className="alert alert--error" role="alert">
                        <AlertCircle size={18} aria-hidden="true" />
                        {error}
                    </div>
                )}

                <form className="login-form" onSubmit={handleSubmit}>
                    <div className="form-group">
                        <label className="form-label" htmlFor="mandatory-current-password">
                            {t("user_settings.current_password")}
                        </label>
                        <input
                            id="mandatory-current-password"
                            className="form-input"
                            type="password"
                            autoComplete="current-password"
                            value={currentPassword}
                            disabled={isSubmitting}
                            maxLength={256}
                            required
                            autoFocus
                            onChange={(event) => setCurrentPassword(event.target.value)}
                        />
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="mandatory-new-password">
                            {t("user_settings.new_password")}
                        </label>
                        <input
                            id="mandatory-new-password"
                            className="form-input"
                            type="password"
                            autoComplete="new-password"
                            aria-describedby="mandatory-password-requirements"
                            value={newPassword}
                            disabled={isSubmitting}
                            minLength={12}
                            maxLength={256}
                            required
                            onChange={(event) => setNewPassword(event.target.value)}
                        />
                        <p id="mandatory-password-requirements" className="form-hint">
                            {t("password_change.requirements")}
                        </p>
                    </div>
                    <div className="form-group">
                        <label className="form-label" htmlFor="mandatory-password-confirmation">
                            {t("user_settings.confirm_password")}
                        </label>
                        <input
                            id="mandatory-password-confirmation"
                            className="form-input"
                            type="password"
                            autoComplete="new-password"
                            value={confirmation}
                            disabled={isSubmitting}
                            minLength={12}
                            maxLength={256}
                            required
                            onChange={(event) => setConfirmation(event.target.value)}
                        />
                    </div>
                    <button type="submit" className="btn btn--primary btn--lg btn--full" disabled={isSubmitting}>
                        {isSubmitting ? <span className="spinner spinner--sm" /> : <KeyRound size={18} aria-hidden="true" />}
                        {t("password_change.submit")}
                    </button>
                    <button type="button" className="btn btn--secondary btn--full" disabled={isSubmitting} onClick={() => void handleLogout()}>
                        <LogOut size={18} aria-hidden="true" />
                        {t("sidebar.logout")}
                    </button>
                </form>
            </section>
        </main>
    );
}
