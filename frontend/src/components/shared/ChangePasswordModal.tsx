import React, { useState } from "react";
import { Save, Key } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { useAuth } from "@/contexts/AuthContext";

export function ChangePasswordModal() {
    const { t } = useLanguage();
    const { user, updateUser, logout } = useAuth();
    const [newPassword, setNewPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");
    const [error, setError] = useState("");
    const [isSubmitting, setIsSubmitting] = useState(false);

    if (!user?.must_change_password) {
        return null;
    }

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        setError("");

        if (newPassword !== confirmPassword) {
            setError(t("user_settings.password_mismatch") || "Les mots de passe ne correspondent pas");
            return;
        }

        if (newPassword.length < 12) {
            setError(t("user_settings.password_min_length_12") || "Le mot de passe doit contenir au moins 12 caractères");
            return;
        }

        setIsSubmitting(true);

        try {
            const response = await fetch("/api/v1/auth/password", {
                method: "PUT",
                headers: {
                    "Content-Type": "application/json",
                    "Authorization": `Bearer ${localStorage.getItem("token")}`,
                },
                body: JSON.stringify({ new_password: newPassword }),
            });

            if (!response.ok) {
                const data = await response.json();
                throw new Error(data.error || t("common.error"));
            }

            // Success: update user context to remove the flag
            updateUser({ must_change_password: false });
        } catch (err: any) {
            const msg = err instanceof Error ? err.message : "Erreur";
            setError(msg.includes(".") ? t(msg) : msg);
        } finally {
            setIsSubmitting(false);
        }
    };

    return (
        <div className="modal-overlay">
            <div className="modal modal--danger">
                <div className="modal-header">
                    <Key size={20} className="text-warning" />
                    <h3>{t("auth.change_password_required") || "Changement de mot de passe requis"}</h3>
                </div>
                
                <div className="modal-content">
                    <p className="mb-4 text-secondary">
                        {t("auth.change_password_required_desc") || "Pour des raisons de sécurité, vous devez changer votre mot de passe avant de continuer."}
                    </p>

                    <form onSubmit={handleSubmit} className="form-column">
                        {error && (
                            <div className="alert alert--error mb-4">
                                {error}
                            </div>
                        )}

                        <div className="form-group">
                            <label className="form-label">{t("user_settings.new_password")}</label>
                            <input
                                type="password"
                                className="form-input"
                                value={newPassword}
                                onChange={(e) => setNewPassword(e.target.value)}
                                required
                                minLength={12}
                                placeholder="••••••••••••"
                            />
                        </div>

                        <div className="form-group">
                            <label className="form-label">{t("user_settings.confirm_password")}</label>
                            <input
                                type="password"
                                className="form-input"
                                value={confirmPassword}
                                onChange={(e) => setConfirmPassword(e.target.value)}
                                required
                                minLength={12}
                                placeholder="••••••••••••"
                            />
                        </div>

                        <div className="modal-footer">
                            <button 
                                type="button" 
                                onClick={logout}
                                className="btn btn--ghost"
                            >
                                {t("auth.logout") || "Déconnexion"}
                            </button>
                            <button 
                                type="submit" 
                                className="btn btn--primary"
                                disabled={isSubmitting || !newPassword}
                            >
                                {isSubmitting ? t("common.loading") : (
                                    <>
                                        <Save size={16} />
                                        {t("common.save") || "Enregistrer"}
                                    </>
                                )}
                            </button>
                        </div>
                    </form>
                </div>
            </div>
            <style>{`
                .modal-overlay {
                    position: fixed;
                    top: 0;
                    left: 0;
                    right: 0;
                    bottom: 0;
                    background: rgba(0, 0, 0, 0.7);
                    display: flex;
                    align-items: center;
                    justify-content: center;
                    z-index: 9999;
                    backdrop-filter: blur(4px);
                }
                .modal {
                    background: var(--bg-secondary);
                    border: 1px solid var(--border-color);
                    border-radius: var(--radius-lg);
                    width: 100%;
                    max-width: 450px;
                    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04);
                    overflow: hidden;
                }
                .modal-header {
                    padding: 1.5rem;
                    border-bottom: 1px solid var(--border-color);
                    display: flex;
                    align-items: center;
                    gap: 12px;
                }
                .modal-header h3 {
                    margin: 0;
                    font-size: 1.125rem;
                    font-weight: 600;
                }
                .modal-content {
                    padding: 1.5rem;
                }
                .modal-footer {
                    display: flex;
                    justify-content: flex-end;
                    gap: 12px;
                    margin-top: 1.5rem;
                }
                .text-warning { color: var(--warning-color, #f59e0b); }
                .text-secondary { color: var(--text-secondary); }
            `}</style>
        </div>
    );
}
