import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
    plugins: [react()],
    resolve: {
        alias: {
            "@": path.resolve(__dirname, "./src"),
        },
    },
    server: {
        port: 3000,
        proxy: {
            "/api": {
                target: "http://localhost:5500",
                changeOrigin: true,
                ws: true, // Enable WS support for /api/v1/ws/...
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
});
