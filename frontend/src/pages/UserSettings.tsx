import { Globe, KeyRound, Laptop, Palette, Save, Shield, UserRound, XCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button, ColorPicker } from "@/components/ui";
import { useAuth } from "@/contexts/AuthContext";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useTheme } from "@/contexts/ThemeContext";
import { useToast } from "@/contexts/ToastContext";
import type { SessionInfo } from "@/schemas/api";
import { apiService } from "@/services";
import { passwordMeetsPolicy } from "@/utils/password";
import { roleLabel } from "@/utils/roles";

export default function UserSettings() {
    const { accentColor, setAccentColor } = useTheme();
    const { user, changePassword, updateUser } = useAuth();
    const { language, setLanguage, t } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { confirm } = useDialog();
    const toast = useToast();
    const [newPassword, setNewPassword] = useState("");
    const [currentPassword, setCurrentPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");
    const [passwordError, setPasswordError] = useState("");
    const [preferenceError, setPreferenceError] = useState("");
    const [savingPreferences, setSavingPreferences] = useState(false);
    const [sessions, setSessions] = useState<SessionInfo[]>([]);
    const [loadingSessions, setLoadingSessions] = useState(true);
    const [sessionError, setSessionError] = useState("");
    const [revokingSession, setRevokingSession] = useState<string | null>(null);

    useEffect(() => setPageTitle(t("user_settings.title"), t("user_settings.subtitle")), [setPageTitle, t]);

    const loadSessions = useCallback(async () => {
        setLoadingSessions(true);
        const response = await apiService.auth.sessions();
        if (response.success) {
            setSessions(response.data);
            setSessionError("");
        } else setSessionError(response.error.message);
        setLoadingSessions(false);
    }, []);

    useEffect(() => { void loadSessions(); }, [loadSessions]);

    const savePreferences = async () => {
        setSavingPreferences(true);
        setPreferenceError("");
        const response = await apiService.auth.updatePreferences({ language, accent_color: accentColor });
        setSavingPreferences(false);
        if (!response.success) return setPreferenceError(response.error.message);
        updateUser(response.data);
        toast.success(t("user_settings.preferences_saved"));
    };

    const handlePasswordChange = async (event: React.FormEvent) => {
        event.preventDefault();
        setPasswordError("");
        if (newPassword !== confirmPassword) return setPasswordError(t("user_settings.password_mismatch"));
        if (!passwordMeetsPolicy(newPassword)) return setPasswordError(t("user_settings.password_min_length"));
        if (newPassword === currentPassword) return setPasswordError(t("user_settings.password_must_differ"));
        try {
            await changePassword(currentPassword, newPassword);
        } catch (error) {
            const message = error instanceof Error ? error.message : t("user_settings.password_error");
            setPasswordError(message.includes(".") ? t(message) : message);
        }
    };

    const revokeSession = async (session: SessionInfo) => {
        if (session.is_current || !await confirm(t("user_settings.revoke_session_confirm"), { title: t("user_settings.revoke_session"), confirmLabel: t("user_settings.revoke_session"), isDestructive: true })) return;
        setRevokingSession(session.id);
        const response = await apiService.auth.revokeSession(session.id);
        setRevokingSession(null);
        if (!response.success) return setSessionError(response.error.message);
        setSessions((current) => current.filter((item) => item.id !== session.id));
        toast.success(t("user_settings.session_revoked"));
    };

    const revokeOthers = async () => {
        if (!await confirm(t("user_settings.revoke_others_confirm"), { title: t("user_settings.revoke_others"), confirmLabel: t("user_settings.revoke_others"), isDestructive: true })) return;
        setRevokingSession("others");
        const response = await apiService.auth.revokeOtherSessions();
        setRevokingSession(null);
        if (!response.success) return setSessionError(response.error.message);
        setSessions((current) => current.filter((session) => session.is_current));
        toast.success(t("user_settings.sessions_revoked"));
    };

    const locale = language === "fr" ? "fr-FR" : "en-US";

    return (
        <div className="account-page">
            <section className="account-section card">
                <header><UserRound size={20} /><div><h2>{t("user_settings.identity")}</h2><p>{t("user_settings.identity_hint")}</p></div></header>
                <div className="account-identity">
                    <div className="user-profile__avatar">{user?.username.charAt(0).toUpperCase()}</div>
                    <div><strong>{user?.username}</strong><span>{user ? roleLabel(user.role, undefined, t) : ""}</span></div>
                </div>
            </section>

            <section className="account-section card">
                <header><Palette size={20} /><div><h2>{t("user_settings.appearance")}</h2><p>{t("user_settings.appearance_hint")}</p></div></header>
                {preferenceError && <div className="alert alert--error" role="alert">{preferenceError}</div>}
                <div className="account-preferences">
                    <div className="form-group"><label className="form-label"><Globe size={16} />{t("settings.language")}</label><div className="language-selector"><button type="button" aria-pressed={language === "fr"} className={`btn ${language === "fr" ? "btn--primary" : "btn--secondary"}`} onClick={() => setLanguage("fr")}>Français</button><button type="button" aria-pressed={language === "en"} className={`btn ${language === "en" ? "btn--primary" : "btn--secondary"}`} onClick={() => setLanguage("en")}>English</button></div></div>
                    <div className="form-group"><label className="form-label">{t("user_settings.accent_color")}</label><ColorPicker value={accentColor} onChange={setAccentColor} /></div>
                </div>
                <footer><Button onClick={() => void savePreferences()} isLoading={savingPreferences} icon={<Save size={16} />}>{t("common.save")}</Button></footer>
            </section>

            <section className="account-section card">
                <header><KeyRound size={20} /><div><h2>{t("user_settings.security")}</h2><p>{t("user_settings.security_hint")}</p></div></header>
                <form onSubmit={handlePasswordChange} className="account-password-form">
                    {passwordError && <div className="alert alert--error" role="alert">{passwordError}</div>}
                    <div className="form-grid form-grid--3col"><div className="form-group"><label className="form-label" htmlFor="current-password">{t("user_settings.current_password")}</label><input id="current-password" type="password" className="input" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} autoComplete="current-password" required maxLength={256} /></div><div className="form-group"><label className="form-label" htmlFor="new-password">{t("user_settings.new_password")}</label><input id="new-password" type="password" className="input" value={newPassword} onChange={(event) => setNewPassword(event.target.value)} autoComplete="new-password" required minLength={12} maxLength={256} /></div><div className="form-group"><label className="form-label" htmlFor="confirm-new-password">{t("user_settings.confirm_password")}</label><input id="confirm-new-password" type="password" className="input" value={confirmPassword} onChange={(event) => setConfirmPassword(event.target.value)} autoComplete="new-password" required minLength={12} maxLength={256} /></div></div>
                    <Button type="submit" variant="secondary" icon={<Shield size={16} />}>{t("user_settings.change_password")}</Button>
                </form>
            </section>

            <section className="account-section card">
                <header><Laptop size={20} /><div><h2>{t("user_settings.sessions")}</h2><p>{t("user_settings.sessions_hint")}</p></div>{sessions.filter((session) => !session.is_current).length > 0 && <Button variant="danger" size="sm" isLoading={revokingSession === "others"} onClick={() => void revokeOthers()}>{t("user_settings.revoke_others")}</Button>}</header>
                {sessionError && <div className="alert alert--error" role="alert">{sessionError}</div>}
                {loadingSessions ? <div className="account-sessions__loading"><span className="spinner" />{t("common.loading")}</div> : <div className="account-sessions">{sessions.map((session) => <article key={session.id} className="account-session"><Laptop size={19} /><div><strong>{session.browser}</strong><span>{t("user_settings.created_session")} {new Date(session.created_at).toLocaleString(locale)} · {t("user_settings.last_activity")} {new Date(session.last_seen_at).toLocaleString(locale)} · {t("user_settings.expires")} {new Date(session.expires_at).toLocaleString(locale)}</span></div>{session.is_current ? <span className="badge badge--success">{t("user_settings.current_session")}</span> : <Button variant="ghost" size="icon" aria-label={t("user_settings.revoke_session")} isLoading={revokingSession === session.id} onClick={() => void revokeSession(session)}><XCircle size={18} /></Button>}</article>)}</div>}
            </section>
        </div>
    );
}
