import { readFile, rename, rm } from "node:fs/promises";
import { resolve } from "node:path";

type JsonObject = Record<string, unknown>;

class ProcessFailure extends Error {
    constructor(message: string, readonly exitCode: number) {
        super(message);
    }
}

const frontendDirectory = resolve(import.meta.dir, "..");
const backendDirectory = resolve(frontendDirectory, "../backend");
const openApiPath = resolve(frontendDirectory, "openapi.json");
const generatedTypesPath = resolve(frontendDirectory, "src/generated/api.ts");

function temporaryPath(target: string): string {
    return `${target}.${process.pid}.${Date.now()}.tmp`;
}

function isObject(value: unknown): value is JsonObject {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validateOpenApi(source: string): void {
    let document: unknown;
    try {
        document = JSON.parse(source);
    } catch {
        throw new Error("The backend produced invalid OpenAPI JSON.");
    }
    if (!isObject(document)
        || document.openapi !== "3.1.0"
        || !isObject(document.info)
        || typeof document.info.title !== "string"
        || !isObject(document.paths)
        || !isObject(document.paths["/openapi.json"])
        || !isObject(document.components)
        || !isObject(document.components.schemas)) {
        throw new Error("The backend produced an incomplete OpenAPI document.");
    }
}

async function refreshSchema(): Promise<void> {
    const temporary = temporaryPath(openApiPath);
    try {
        const processHandle = Bun.spawn(
            ["cargo", "run", "--locked", "--quiet", "--", "--print-openapi"],
            {
                cwd: backendDirectory,
                stdout: "pipe",
                stderr: "inherit",
            },
        );
        const [exitCode, output] = await Promise.all([
            processHandle.exited,
            new Response(processHandle.stdout).text(),
        ]);
        if (exitCode !== 0) {
            throw new ProcessFailure(`Cargo OpenAPI generation failed with exit code ${exitCode}.`, exitCode);
        }
        validateOpenApi(output);
        await Bun.write(temporary, output);
        await rename(temporary, openApiPath);
    } finally {
        await rm(temporary, { force: true });
    }
}

async function generateTypes(): Promise<void> {
    const temporary = temporaryPath(generatedTypesPath);
    try {
        validateOpenApi(await readFile(openApiPath, "utf8"));
        const processHandle = Bun.spawn(
            [process.execPath, "x", "openapi-typescript", openApiPath, "-o", temporary],
            {
                cwd: frontendDirectory,
                stdout: "inherit",
                stderr: "inherit",
            },
        );
        const exitCode = await processHandle.exited;
        if (exitCode !== 0) {
            throw new ProcessFailure(`Type generation failed with exit code ${exitCode}.`, exitCode);
        }
        const generated = await readFile(temporary, "utf8");
        if (generated.length < 1_024
            || !generated.includes("export interface paths")
            || !generated.includes("export interface components")) {
            throw new Error("openapi-typescript produced an incomplete TypeScript contract.");
        }
        await rename(temporary, generatedTypesPath);
    } finally {
        await rm(temporary, { force: true });
    }
}

try {
    const action = process.argv[2];
    if (action === "refresh") {
        await refreshSchema();
    } else if (action === "generate") {
        await generateTypes();
    } else {
        throw new Error("Usage: bun run scripts/openapi.ts <refresh|generate>");
    }
} catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = error instanceof ProcessFailure ? error.exitCode : 1;
}
