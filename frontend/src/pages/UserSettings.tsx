import { useState, useEffect } from 'react';
import { Save, Palette, Key, User, Link2, Globe } from 'lucide-react';
import { useTheme } from '../contexts/ThemeContext';
import { useAuth } from '../contexts/AuthContext';
import { useLanguage } from '../contexts/LanguageContext';
import { usePageTitle } from '../contexts/PageTitleContext';
import { ColorPicker, LoadingScreen } from '../components/common';

export default function UserSettings() {
    const { accentColor, setAccentColor } = useTheme();
    const { user, updateUser } = useAuth();
    const { language, setLanguage, t } = useLanguage();
    const [isLoading, setIsLoading] = useState(true);
    const [newPassword, setNewPassword] = useState('');
    const [confirmPassword, setConfirmPassword] = useState('');
    const [passwordError, setPasswordError] = useState('');
    const [passwordSuccess, setPasswordSuccess] = useState(false);
    const [isSavingColor, setIsSavingColor] = useState(false);
    const [colorSaveSuccess, setColorSaveSuccess] = useState(false);
    const [originalColor, setOriginalColor] = useState(accentColor);
    const hasColorChanged = accentColor.toLowerCase() !== originalColor.toLowerCase();

    useEffect(() => {
        setTimeout(() => setIsLoading(false), 300);
    }, []);

    const { setPageTitle } = usePageTitle();
    useEffect(() => {
        setPageTitle(t('user_settings.title'), t('user_settings.subtitle'));
    }, [setPageTitle, t]);

    const handlePasswordChange = async (e: React.FormEvent) => {
        e.preventDefault();
        setPasswordError('');
        setPasswordSuccess(false);

        if (newPassword !== confirmPassword) {
            setPasswordError(t('user_settings.password_mismatch'));
            return;
        }

        if (newPassword.length < 6) {
            setPasswordError(t('user_settings.password_min_length'));
            return;
        }

        try {
            const response = await fetch('/api/v1/auth/password', {
                method: 'PUT',
                headers: {
                    'Content-Type': 'application/json',
                    'Authorization': `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify({ new_password: newPassword }),
            });

            if (!response.ok) {
                const data = await response.json();
                throw new Error(data.error || t('common.error'));
            }

            setPasswordSuccess(true);
            setNewPassword('');
            setConfirmPassword('');
        } catch (err: any) {
            // If err.message is a key (no spaces), translate it. Otherwise use it as is?
            // Actually backend sends keys now. But catch might catch other things.
            // We can safely try t(err.message). If no key, it returns the key.
            const msg = err instanceof Error ? err.message : t('user_settings.password_error');
            // Quick heuristic: if it looks like a key (contains dot), translate it
            setPasswordError(msg.includes('.') ? t(msg) : msg);
        }
    };

    const handleSaveColor = async () => {
        if (!user) return;
        setIsSavingColor(true);
        setColorSaveSuccess(false);

        try {
            const response = await fetch(`/api/v1/users/${user.id}`, {
                method: 'PUT',
                headers: {
                    'Content-Type': 'application/json',
                    'Authorization': `Bearer ${localStorage.getItem('token')}`,
                },
                body: JSON.stringify({ accent_color: accentColor }),
            });

            if (!response.ok) {
                throw new Error(t('user_settings.color_error'));
            }

            setOriginalColor(accentColor);
            setColorSaveSuccess(true);
            // Update user in AuthContext (syncs state + localStorage)
            updateUser({ accent_color: accentColor });
            setTimeout(() => setColorSaveSuccess(false), 3000);
        } catch (err) {
            console.error('Erreur:', err);
        } finally {
            setIsSavingColor(false);
        }
    };

    if (isLoading) {
        return <LoadingScreen />;
    }

    return (
        <div>

            <div className="settings-grid">
                {/* Profile Info */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <User size={20} />
                        {t('user_settings.profile')}
                    </h3>

                    <div className="user-profile">
                        <div className="user-profile__avatar">
                            {user?.username.charAt(0).toUpperCase()}
                        </div>
                        <div className="user-profile__info">
                            <span className="user-profile__name">{user?.username}</span>
                            <span className="user-profile__role">
                                {user?.role === 'admin' ? t('user_settings.role_admin') : t('user_settings.role_user')}
                            </span>
                        </div>
                    </div>
                </div>

                {/* Language (Before Perso) */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <Globe size={20} />
                        {t('settings.language')}
                    </h3>

                    <div className="form-group">
                        <label className="form-label">{t('settings.select_language')}</label>
                        <div className="language-selector">
                            <button
                                className={`btn ${language === 'fr' ? 'btn--primary' : 'btn--secondary'}`}
                                onClick={() => setLanguage('fr')}
                            >
                                🇫🇷 Français
                            </button>
                            <button
                                className={`btn ${language === 'en' ? 'btn--primary' : 'btn--secondary'}`}
                                onClick={() => setLanguage('en')}
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
                        {t('settings.theme')}
                    </h3>

                    <div className="form-group">
                        <label className="form-label">{t('user_settings.accent_color')}</label>
                        <ColorPicker value={accentColor} onChange={setAccentColor} />
                        {colorSaveSuccess && (
                            <p className="form-hint text-success mt-2">
                                {t('user_settings.color_saved')}
                            </p>
                        )}
                    </div>

                    <button
                        onClick={handleSaveColor}
                        disabled={!hasColorChanged || isSavingColor}
                        className="btn btn--primary mt-4"
                    >
                        <Save size={18} />
                        {isSavingColor ? t('user_settings.data_saving') : t('user_settings.save_color')}
                    </button>
                </div>

                {/* Discord Link */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                            <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515a.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0a12.64 12.64 0 0 0-.617-1.25a.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057a19.9 19.9 0 0 0 5.993 3.03a.078.078 0 0 0 .084-.028a14.09 14.09 0 0 0 1.226-1.994a.076.076 0 0 0-.041-.106a13.107 13.107 0 0 1-1.872-.892a.077.077 0 0 1-.008-.128a10.2 10.2 0 0 0 .372-.292a.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127a12.299 12.299 0 0 1-1.873.892a.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028a19.839 19.839 0 0 0 6.002-3.03a.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z" />
                        </svg>
                        {t('user_settings.discord_link')}
                    </h3>

                    <p className="form-hint mb-4">
                        {t('user_settings.discord_desc')}
                    </p>

                    <div className="discord-link">
                        <div className="discord-link__status discord-link__status--disconnected">
                            <Link2 size={18} />
                            <span>{t('user_settings.discord_disconnected')}</span>
                        </div>
                        <button className="btn btn--discord">
                            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                                <path d="M20.317 4.37a19.791 19.791 0 0 0-4.885-1.515a.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.27 18.27 0 0 0-5.487 0a12.64 12.64 0 0 0-.617-1.25a.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.082.082 0 0 0 .031.057a19.9 19.9 0 0 0 5.993 3.03a.078.078 0 0 0 .084-.028a14.09 14.09 0 0 0 1.226-1.994a.076.076 0 0 0-.041-.106a13.107 13.107 0 0 1-1.872-.892a.077.077 0 0 1-.008-.128a10.2 10.2 0 0 0 .372-.292a.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.01c.12.098.246.198.373.292a.077.077 0 0 1-.006.127a12.299 12.299 0 0 1-1.873.892a.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028a19.839 19.839 0 0 0 6.002-3.03a.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03z" />
                            </svg>
                            {t('user_settings.connect_discord')}
                        </button>
                    </div>
                </div>

                {/* Change Password */}
                <div className="card">
                    <h3 className="settings-section__title">
                        <Key size={20} />
                        {t('user_settings.change_password')}
                    </h3>

                    <form onSubmit={handlePasswordChange}>
                        {passwordError && (
                            <div className="alert alert--error mb-4">
                                {passwordError}
                            </div>
                        )}

                        {passwordSuccess && (
                            <div className="alert alert--success mb-4">
                                {t('user_settings.password_success')}
                            </div>
                        )}

                        <div className="form-grid form-grid--2col">
                            <div className="form-group">
                                <label className="form-label">{t('user_settings.new_password')}</label>
                                <input
                                    type="password"
                                    placeholder="••••••••"
                                    className="form-input"
                                    value={newPassword}
                                    onChange={(e) => setNewPassword(e.target.value)}
                                />
                            </div>
                            <div className="form-group">
                                <label className="form-label">{t('user_settings.confirm_password')}</label>
                                <input
                                    type="password"
                                    placeholder="••••••••"
                                    className="form-input"
                                    value={confirmPassword}
                                    onChange={(e) => setConfirmPassword(e.target.value)}
                                />
                            </div>
                        </div>

                        <button type="submit" className="btn btn--secondary">
                            <Save size={18} />
                            {t('user_settings.change_password')}
                        </button>
                    </form>
                </div>
            </div>
        </div>
    );
}
