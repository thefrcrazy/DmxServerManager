-- Verdict de mise à jour persistant.
--
-- Il n'existait qu'en mémoire, calculé à la demande à l'ouverture d'une page :
-- l'utilisateur attendait le lancement de SteamCMD ou du téléchargeur officiel,
-- et le résultat disparaissait au redémarrage du panneau. Le conserver permet
-- de répondre instantanément, de l'afficher sur la liste des serveurs sans
-- déclencher autant de vérifications qu'il y a d'instances, et de laisser une
-- tâche de fond le rafraîchir.
CREATE TABLE instance_update_checks (
    instance_id TEXT PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    state TEXT NOT NULL
        CHECK (state IN ('not_installed', 'up_to_date', 'update_available', 'check_failed')),
    installed_version TEXT,
    installed_build TEXT,
    available_version TEXT,
    available_build TEXT,
    -- Identité de ce qui a été comparé : un changement de profil, de réglages ou
    -- de version installée invalide le verdict sans attendre son expiration.
    fingerprint TEXT NOT NULL,
    checked_at TEXT NOT NULL
);

CREATE INDEX idx_instance_update_checks_state ON instance_update_checks(state);
