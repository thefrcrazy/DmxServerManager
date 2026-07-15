/// <reference types="vite/client" />

interface ImportMetaEnv {
    readonly VITE_BACKEND_HTTPS?: "true" | "false"
}

interface ImportMeta {
    readonly env: ImportMetaEnv
}
