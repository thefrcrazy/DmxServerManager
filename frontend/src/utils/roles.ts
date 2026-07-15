export type Translator = (key: string) => string;

const SYSTEM_ROLE_KEYS: Record<string, string> = {
    owner: "administration.roles.system.owner",
    admin: "administration.roles.system.admin",
    operator: "administration.roles.system.operator",
    viewer: "administration.roles.system.viewer",
};

export function roleLabel(roleId: string, fallback: string | undefined, t: Translator): string {
    const key = SYSTEM_ROLE_KEYS[roleId];
    return key ? t(key) : fallback ?? roleId;
}

export function permissionLabel(permission: string, t: Translator): string {
    const key = `administration.permissions.${permission.replaceAll(".", "_")}`;
    const translated = t(key);
    return translated === key ? permission : translated;
}

export function translatedError(error: unknown, fallback: string, t: Translator): string {
    if (!(error instanceof Error)) return fallback;
    const translated = t(error.message);
    return translated === error.message ? error.message : translated;
}
