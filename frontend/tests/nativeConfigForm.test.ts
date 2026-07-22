import { describe, expect, test } from "bun:test";
import {
    parseNativeConfig,
    type NativeConfigField,
    type NativeConfigModel,
    type NativeConfigScalar,
} from "../src/utils/nativeConfigForm";

function field(model: NativeConfigModel, label: string): NativeConfigField {
    const match = model.fields.find((candidate) => candidate.label === label);
    if (!match) throw new Error(`Missing field ${label}`);
    return match;
}

function changed(model: NativeConfigModel, updates: Record<string, NativeConfigScalar>): Record<string, NativeConfigScalar> {
    return Object.fromEntries(model.fields.map((item) => [item.id, updates[item.label] ?? item.value]));
}

describe("formulaires de configuration natifs", () => {
    test("modifie les primitives JSON tout en conservant les clés inconnues et les secrets", () => {
        const source = `{
  "Version": 4,
  "ServerName": "Hytale Server",
  "Password": "do-not-expose",
  "Defaults": {
    "World": "default",
    "GameMode": "Adventure"
  },
  "Modules": {}
}\n`;
        const model = parseNativeConfig("json", source);
        const password = field(model, "Password");
        expect(password.kind).toBe("secret");
        expect(password.value).toBe("");
        expect(password.configured).toBe(true);

        const serialized = model.serialize(changed(model, { ServerName: "Serveur Max", GameMode: "Creative" }));
        const parsed = JSON.parse(serialized);
        expect(parsed.ServerName).toBe("Serveur Max");
        expect(parsed.Password).toBe("do-not-expose");
        expect(parsed.Defaults.GameMode).toBe("Creative");
        expect(parsed.Modules).toEqual({});
        expect(serialized.endsWith("\n")).toBe(true);
    });

    test("préserve les commentaires et la mise en forme des properties", () => {
        const source = "# Réglages publics\nmotd=Serveur DMX # commentaire conservé\nmax-players=20\nonline-mode=true\nrcon.password=secret-value\n";
        const model = parseNativeConfig("properties", source);
        const serialized = model.serialize(changed(model, {
            motd: "Serveur E2E",
            "max-players": 32,
            "online-mode": false,
        }));

        expect(serialized).toContain("# Réglages publics");
        expect(serialized).toContain("motd=Serveur E2E # commentaire conservé");
        expect(serialized).toContain("max-players=32");
        expect(serialized).toContain("online-mode=false");
        expect(serialized).toContain("rcon.password=secret-value");
    });

    test("extrait le tuple Palworld et les commandes Rust sans réécrire le reste", () => {
        const palworld = "OptionSettings=(ServerName=\"Palworld\",ServerPlayerMaxNum=32,AdminPassword=\"secret\",bIsPvP=False)\n";
        const palworldModel = parseNativeConfig("ini", palworld);
        const palworldResult = palworldModel.serialize(changed(palworldModel, {
            ServerName: "Monde Max",
            ServerPlayerMaxNum: 24,
            bIsPvP: true,
        }));
        expect(palworldResult).toBe("OptionSettings=(ServerName=\"Monde Max\",ServerPlayerMaxNum=24,AdminPassword=\"secret\",bIsPvP=True)\n");

        const rust = "server.hostname \"Rust Lab\"\nserver.maxplayers 50\nserver.description \"Survie\" # public\n";
        const rustModel = parseNativeConfig("properties", rust);
        const rustResult = rustModel.serialize(changed(rustModel, { "server.hostname": "Rust Pro", "server.maxplayers": 75 }));
        expect(rustResult).toContain("server.hostname \"Rust Pro\"");
        expect(rustResult).toContain("server.maxplayers 75");
        expect(rustResult).toContain("# public");
    });

    test("modifie les propriétés XML en échappant leur valeur", () => {
        const source = "<ServerSettings>\n  <property name=\"ServerName\" value=\"DMX\"/>\n  <property name=\"MaxPlayers\" value=\"8\"/>\n  <property name=\"AdminPassword\" value=\"a&amp;b\"/>\n</ServerSettings>\n";
        const model = parseNativeConfig("xml", source);
        const serialized = model.serialize(changed(model, { ServerName: "Max & Friends", MaxPlayers: 12 }));
        expect(serialized).toContain("value=\"Max &amp; Friends\"");
        expect(serialized).toContain("name=\"MaxPlayers\" value=\"12\"");
        expect(serialized).toContain("name=\"AdminPassword\" value=\"a&amp;b\"");
    });
});
