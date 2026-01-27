import { useState, FormEvent, useEffect } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import { useLanguage } from '../contexts/LanguageContext';
import { LogIn, UserPlus, Rocket, AlertCircle } from 'lucide-react';

interface LoginSettings {
  login_background_url?: string;
  login_default_color?: string;
}

export default function Login() {
  const { user, login } = useAuth();
  const { t } = useLanguage();
  const navigate = useNavigate();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [needsSetup, setNeedsSetup] = useState<boolean | null>(null);
  const [checkingStatus, setCheckingStatus] = useState(true);
  const [loginSettings, setLoginSettings] = useState<LoginSettings>({});

  useEffect(() => {
    checkSetupStatus();
    fetchLoginSettings();
  }, []);

  // Apply custom background if set
  useEffect(() => {
    if (loginSettings.login_background_url) {
      document.documentElement.style.setProperty(
        '--login-bg-image',
        `url(${loginSettings.login_background_url})`
      );
    }
    if (loginSettings.login_default_color) {
      document.documentElement.style.setProperty(
        '--color-accent',
        loginSettings.login_default_color
      );
    }
    return () => {
      // Cleanup: remove custom background on unmount
      document.documentElement.style.removeProperty('--login-bg-image');
    };
  }, [loginSettings]);

  const fetchLoginSettings = async () => {
    try {
      const response = await fetch('/api/v1/settings');
      if (response.ok) {
        const data = await response.json();
        setLoginSettings({
          login_background_url: data.login_background_url,
          login_default_color: data.login_default_color,
        });
      }
    } catch (err) {
      console.error('Failed to fetch login settings:', err);
    }
  };

  const checkSetupStatus = async () => {
    try {
      const response = await fetch('/api/v1/auth/status');
      if (response.ok) {
        const data = await response.json();
        setNeedsSetup(data.needs_setup);
      }
    } catch (err) {
      console.error('Failed to check setup status:', err);
      setNeedsSetup(false);
    } finally {
      setCheckingStatus(false);
    }
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');

    if (needsSetup && password !== confirmPassword) {
      setError(t('user_settings.password_mismatch'));
      return;
    }

    setIsLoading(true);

    try {
      if (needsSetup) {
        const response = await fetch('/api/v1/auth/register', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ username, password }),
        });

        if (!response.ok) {
          const data = await response.json();
          throw new Error(data.error || 'common.error');
        }

        const data = await response.json();
        localStorage.setItem('token', data.token);
        localStorage.setItem('user', JSON.stringify(data.user));
        window.location.href = '/dashboard';
      } else {
        await login(username, password);
        navigate('/dashboard');
      }
    } catch (err) {
      setError(err instanceof Error ? t(err.message) : t('auth.login_failed'));
    } finally {
      setIsLoading(false);
    }
  };

  if (user) {
    return <Navigate to="/dashboard" replace />;
  }

  // Redirect to setup wizard if first installation
  if (needsSetup === true) {
    return <Navigate to="/setup" replace />;
  }

  if (checkingStatus) {
    return (
      <div className="loading-screen">
        <div className="spinner"></div>
        <p className="text-muted">{t('common.loading')}</p>
      </div>
    );
  }

  return (
    <div className="login-page">
      <div className="card login-card">
        <div className="login-header">
          <img
            src="/draveur-manager-logo.png"
            alt="Draveur Manager"
            className="login-header__logo"
          />
          <p className="text-muted">
            {needsSetup
              ? t('auth.setup_admin')
              : t('auth.login_subtitle')}
          </p>
        </div>

        {needsSetup && (
          <div className="login-setup-badge">
            <Rocket size={18} />
            <span>{t('auth.first_install')}</span>
          </div>
        )}

        <form onSubmit={handleSubmit} className="login-form">
          {error && (
            <div className="alert alert--error">
              <AlertCircle size={16} />
              {error}
            </div>
          )}

          <div className="form-group">
            <label className="form-label">{t('auth.username')}</label>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder={needsSetup ? t('auth.username') : t('auth.username')}
              required
              className="form-input"
            />
          </div>

          <div className="form-group">
            <label className="form-label">{t('auth.password')}</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={needsSetup ? t('auth.password') : t('auth.password')}
              required
              className="form-input"
            />
          </div>

          {needsSetup && (
            <div className="form-group">
              <label className="form-label">{t('auth.confirm_password')}</label>
              <input
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                placeholder={t('auth.confirm_password')}
                required
                className="form-input"
              />
            </div>
          )}

          <button
            type="submit"
            className="btn btn--primary btn--lg btn--full"
            disabled={isLoading}
          >
            {isLoading ? (
              <span className="flex-center">
                <div className="spinner spinner--sm spinner--light"></div>
                {t('common.loading')}
              </span>
            ) : (
              <span className="flex-center">
                {needsSetup ? <UserPlus size={18} /> : <LogIn size={18} />}
                {needsSetup ? t('auth.register') : t('auth.login')}
              </span>
            )}
          </button>
        </form>

        {!needsSetup && (
          <div className="login-footer">
            <p>Draveur Manager v0.1.0</p>
          </div>
        )}
      </div>
    </div>
  );
}
