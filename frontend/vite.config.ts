import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

const rootDirectory = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig(({ mode }) => {
    const env = loadEnv(mode, process.cwd(), "VITE_");
    const backendProtocol = env.VITE_BACKEND_HTTPS === "true" ? "https" : "http";

    return {
        plugins: [react()],
        resolve: {
            alias: {
                "@": path.resolve(rootDirectory, "src"),
            },
        },
        server: {
            port: 3000,
            proxy: {
                "/api": {
                    target: `${backendProtocol}://localhost:5500`,
                    changeOrigin: true,
                    secure: false,
                },
            },
        },
        css: {
            preprocessorOptions: {
                scss: {
                    additionalData: "@use \"@/styles/_variables\" as *; @use \"@/styles/_mixins\" as *;",
                },
            },
        },
    };
});
