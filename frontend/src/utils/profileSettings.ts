import { GameProfile } from "@/schemas/api";

export type ProfileValue = string | number | boolean | null | ProfileValue[] | { [key: string]: ProfileValue };

export function initialProfileSettings(profile: GameProfile): Record<string, ProfileValue> {
    return Object.fromEntries(
        Object.entries(profile.settings_schema.properties)
            .filter(([, property]) => !property.secret)
            .map(([key, property]) => {
                if (Array.isArray(property.default)) {
                    return [key, property.default as ProfileValue[]];
                }
                if (property.default !== undefined && ["string", "number", "boolean"].includes(typeof property.default)) {
                    return [key, property.default as ProfileValue];
                }
                if (property.type === "boolean") return [key, false];
                if (property.type === "array") return [key, []];
                if (property.type === "integer" || property.type === "number") return [key, property.minimum ?? 0];
                return [key, ""];
            }),
    );
}

export function partitionProfileValues(
    profile: GameProfile,
    values: Record<string, ProfileValue>,
): { settings: Record<string, ProfileValue>; secrets: Record<string, string> } {
    const settings: Record<string, ProfileValue> = {};
    const secrets: Record<string, string> = {};

    for (const [name, property] of Object.entries(profile.settings_schema.properties)) {
        const value = values[name];
        if (value === undefined) continue;
        if (property.secret || property.writeOnly) {
            const secret = typeof value === "string" ? value : "";
            if (secret) secrets[name] = secret;
        } else {
            if (value === "" && !profile.settings_schema.required.includes(name)) continue;
            settings[name] = value;
        }
    }

    return { settings, secrets };
}

export function isSafeRelativeExecutable(value: string): boolean {
    if (!value || /[\u0000-\u001f\u007f]/u.test(value) || value.includes(":")) return false;
    if (value.startsWith("/") || value.startsWith("\\")) return false;
    return !value.split(/[\\/]/).includes("..");
}
