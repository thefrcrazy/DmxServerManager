import { useState, useEffect } from "react";
import { Save, Palette, Key, User, Globe } from "lucide-react";
import { useTheme } from "../contexts/ThemeContext";
import { useAuth } from "../contexts/AuthContext";
import { useLanguage } from "../contexts/LanguageContext";
import { usePageTitle } from "../contexts/PageTitleContext";
import { ColorPicker } from "@/components/ui";
import { roleLabel } from "@/utils/roles";
import { passwordMeetsPolicy } from "@/utils/password";

export default function UserSettings() {
    const { accentColor, setAccentColor } = useTheme();
    const { user, changePassword } = useAuth();
    const { language, setLanguage, t } = useLanguage();
    const [newPassword, setNewPassword] = useState("");
    const [currentPassword, setCurrentPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");
    const [passwordError, setPasswordError] = useState("");

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t("user_settings.title"), t("user_settings.subtitle"));
    }, [setPageTitle, t]);

    const handlePasswordChange = async (e: React.FormEvent) => {
        e.preventDefault();
        setPasswordError("");

        if (newPassword !== confirmPassword) {
            setPasswordError(t("user_settings.password_mismatch"));
            return;
        }

        if (!passwordMeetsPolicy(newPassword)) {
            setPasswordError(t("user_settings.password_min_length"));
            return;
        }

        if (newPassword === currentPassword) {
            setPasswordError(t("user_settings.password_must_differ"));
            return;
        }

        try {
            await changePassword(currentPassword, newPassword);
            setCurrentPassword("");
            setNewPassword("");
            setConfirmPassword("");
        } catch (err: unknown) {
            const msg = err instanceof Error ? err.message : t("user_settings.password_error");
            setPasswordError(msg.includes(".") ? t(msg) : msg);
        }
    };

    return (
        <div>

            <div className="settings-grid">
                {/* Profile Info */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <User size={20} />
                        {t("user_settings.profile")}
                    </h3>

                    <div className="user-profile">
                        <div className="user-profile__avatar">
                            {user?.username.charAt(0).toUpperCase()}
                        </div>
                        <div className="user-profile__info">
                            <span className="user-profile__name">{user?.username}</span>
                            <span className="user-profile__role">
                                {user ? roleLabel(user.role, undefined, t) : ""}
                            </span>
                        </div>
                    </div>
                </div>

                {/* Language (Before Perso) */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <Globe size={20} />
                        {t("settings.language")}
                    </h3>

                    <div className="form-group">
                        <label className="form-label">{t("settings.select_language")}</label>
                        <div className="language-selector">
                            <button
                                type="button"
                                aria-pressed={language === "fr"}
                                className={`btn ${language === "fr" ? "btn--primary" : "btn--secondary"}`}
                                onClick={() => setLanguage("fr")}
                            >
                                🇫🇷 Français
                            </button>
                            <button
                                type="button"
                                aria-pressed={language === "en"}
                                className={`btn ${language === "en" ? "btn--primary" : "btn--secondary"}`}
                                onClick={() => setLanguage("en")}
                            >
                                🇺🇸 English
                            </button>
                        </div>
                    </div>
                </div>

                {/* Personnalisation */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <Palette size={20} />
                        {t("settings.theme")}
                    </h3>

                    <div className="form-group">
                        <label className="form-label">{t("user_settings.accent_color")}</label>
                        <ColorPicker value={accentColor} onChange={setAccentColor} />
                        <p className="form-hint">{t("user_settings.browser_preference_hint")}</p>
                    </div>
                </div>

                {/* Change Password */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <Key size={20} />
                        {t("user_settings.change_password")}
                    </h3>

                    <form onSubmit={handlePasswordChange}>
                        {passwordError && (
                            <div className="alert alert--error mb-4" role="alert">
                                {passwordError}
                            </div>
                        )}

                        <div className="form-grid form-grid--2col">
                            <div className="form-group">
                                <label className="form-label" htmlFor="current-password">{t("user_settings.current_password")}</label>
                                <input id="current-password" type="password" className="form-input" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} autoComplete="current-password" required maxLength={256} />
                            </div>
                            <div className="form-group">
                                <label className="form-label" htmlFor="new-password">{t("user_settings.new_password")}</label>
                                <input
                                    id="new-password"
                                    type="password"
                                    placeholder="••••••••"
                                    className="form-input"
                                    value={newPassword}
                                    onChange={(e) => setNewPassword(e.target.value)}
                                    autoComplete="new-password"
                                    required
                                    minLength={12}
                                    maxLength={256}
                                />
                            </div>
                            <div className="form-group">
                                <label className="form-label" htmlFor="confirm-new-password">{t("user_settings.confirm_password")}</label>
                                <input
                                    id="confirm-new-password"
                                    type="password"
                                    placeholder="••••••••"
                                    className="form-input"
                                    value={confirmPassword}
                                    onChange={(e) => setConfirmPassword(e.target.value)}
                                    autoComplete="new-password"
                                    required
                                    minLength={12}
                                    maxLength={256}
                                />
                            </div>
                        </div>

                        <button type="submit" className="btn btn--secondary">
                            <Save size={18} />
                            {t("user_settings.change_password")}
                        </button>
                    </form>
                </div>
            </div>
        </div>
    );
}
