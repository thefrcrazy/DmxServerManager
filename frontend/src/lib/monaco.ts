// Monaco is bundled with the application instead of being fetched from a CDN.
// `@monaco-editor/loader` otherwise injects scripts from cdn.jsdelivr.net, which
// the panel Content-Security-Policy (`script-src 'self'`) blocks and which is
// unreachable on the offline and LAN deployments this panel targets.
// Passing `monaco` to `loader.config` short-circuits that injection entirely.
//
// Import specifiers follow the package `exports` map (`./*` -> `./esm/vs/*.js`),
// so they carry no `esm/vs` prefix.
import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";
import JsonWorker from "monaco-editor/languages/features/json/json.worker?worker";

// `label` is the language id for language services and empty for the editor's own
// background work; see the JSON worker manager, which passes `languageId`.
self.MonacoEnvironment = {
    getWorker: (_workerId: string, label: string): Worker =>
        label === "json" ? new JsonWorker() : new EditorWorker(),
};

loader.config({ monaco });

export { monaco };
