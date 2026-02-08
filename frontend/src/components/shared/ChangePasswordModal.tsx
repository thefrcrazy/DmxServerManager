import React, { useState } from "react";
import { Save, Key } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { useAuth } from "@/contexts/AuthContext";
import { Modal, Button, Input } from "@/components/ui";

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

    const footer = (
        <>
            <Button variant="ghost" onClick={logout}>
                {t("auth.logout") || "Déconnexion"}
            </Button>
            <Button
                onClick={handleSubmit}
                disabled={isSubmitting || !newPassword}
                isLoading={isSubmitting}
            >
                <Save size={16} />
                {t("common.save") || "Enregistrer"}
            </Button>
        </>
    );

    const title = (
        <div className="flex items-center gap-2 text-warning">
            <Key size={20} />
            {t("auth.change_password_required") || "Changement de mot de passe requis"}
        </div>
    );

    return (
        <Modal
            isOpen={true}
            onClose={() => {}} // Cannot close without changing password or logging out
            title={title}
            footer={footer}
            isDanger
        >
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
                    <Input
                        type="password"
                        value={newPassword}
                        onChange={(e) => setNewPassword(e.target.value)}
                        required
                        minLength={12}
                        placeholder="••••••••••••"
                    />
                </div>

                <div className="form-group">
                    <label className="form-label">{t("user_settings.confirm_password")}</label>
                    <Input
                        type="password"
                        value={confirmPassword}
                        onChange={(e) => setConfirmPassword(e.target.value)}
                        required
                        minLength={12}
                        placeholder="••••••••••••"
                    />
                </div>
            </form>
        </Modal>
    );
}